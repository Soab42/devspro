import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import type { PageStatus, Resource } from "../../ui";
import { Mono, statusOf, useResource } from "../../ui";
import { PackageX, PlugZap } from "../../ui/icons";
import { isWindows } from "../../app/platform";
import { detectDocker, startDockerEngine } from "./api";
import type { DockerDetection } from "./api";

/** What a blocked page can offer besides Recheck. Each is optional so the
    Cleaner's Docker tab, which has room for one button, can keep just that. */
export interface DockerBlockedActions {
  onRecheck: () => void;
  /** Installed but not running: start Docker Desktop / the docker service. */
  onStart?: () => void;
  starting?: boolean;
  /** Why the last start didn't take, shown under the guidance. */
  startError?: string | null;
  /** Not installed: open the Apps page, which installs it. */
  onOpenApps?: () => void;
}

/**
 * Docker's two precondition failures, as a PageStatus.
 *
 * Shared because the Docker page, the Cleaner's Docker tab, Databases and the
 * Data browser all gate on exactly the same conditions, and previously carried
 * their own near-identical copies of this markup — including the group-membership
 * guidance below, which is load-bearing security information and must not be
 * dropped or reworded away.
 *
 * Returns null when Docker is usable, or when detection hasn't completed yet.
 */
export function dockerBlockedStatus(
  detection: DockerDetection | null,
  actions: DockerBlockedActions,
): PageStatus | null {
  if (!detection) return null;
  const recheck = { label: "Recheck", onClick: actions.onRecheck };

  if (!detection.found) {
    return {
      kind: "blocked",
      icon: PackageX,
      title: "Docker isn't installed",
      message: (
        <>
          {isWindows ? (
            <p>
              This page needs Docker Desktop. Install it from Apps — DevsPro installs the
              latest release through winget, with one administrator prompt. Docker
              Desktop runs on WSL 2, so Windows may ask to restart; afterwards open
              Docker Desktop once to accept its terms, then come back here.
            </p>
          ) : (
            <p>
              This page needs Docker. Install it from Apps — DevsPro installs the
              distribution's Docker engine and Compose, with one administrator prompt.
              Then add your user to the <Mono variant="chip">docker</Mono> group with{" "}
              <Mono variant="chip">sudo usermod -aG docker $USER</Mono> and log out and
              back in, so DevsPro can use it without root.
            </p>
          )}
          <p>
            Or install it yourself, following{" "}
            <a
              href={
                isWindows
                  ? "https://docs.docker.com/desktop/setup/install/windows-install/"
                  : "https://docs.docker.com/engine/install/"
              }
              target="_blank"
              rel="noreferrer"
            >
              docs.docker.com
            </a>
            , then recheck.
          </p>
        </>
      ),
      ...(actions.onOpenApps
        ? {
            action: { label: "Install from Apps", onClick: actions.onOpenApps },
            secondaryAction: recheck,
          }
        : { action: recheck }),
    };
  }

  if (!detection.daemon_reachable) {
    return {
      kind: "blocked",
      icon: PlugZap,
      tone: "warning",
      title: actions.starting ? "Starting Docker…" : "Docker isn't running",
      message: actions.starting ? (
        <p>
          Waiting for the Docker engine to answer. This can take a minute the first
          time; the page opens on its own once it's up.
        </p>
      ) : (
        <>
          {actions.startError && <p>Couldn't start it: {actions.startError}</p>}
          <p>{detection.error ?? "Could not reach the Docker daemon."}</p>
          {isWindows ? (
            // No docker group on Windows: the daemon lives inside Docker Desktop,
            // and it is unreachable because Docker Desktop isn't running yet.
            <p>
              Start Docker Desktop and wait until it reports the engine as running.
              DevsPro never runs Docker commands elevated.
            </p>
          ) : (
            <p>
              If that mentions permission denied, make sure your user is in the{" "}
              <Mono variant="chip">docker</Mono> group — run{" "}
              <Mono variant="chip">sudo usermod -aG docker $USER</Mono>, then log out and
              back in. DevsPro never runs Docker commands elevated.
            </p>
          )}
        </>
      ),
      ...(actions.onStart
        ? {
            action: {
              label: isWindows ? "Start Docker Desktop" : "Start Docker",
              onClick: actions.onStart,
              loading: actions.starting,
            },
            secondaryAction: recheck,
          }
        : { action: recheck }),
    };
  }

  return null;
}

/** How long a start is waited on before the page stops polling and says so. */
const START_TIMEOUT_MS = 120_000;
const START_POLL_MS = 2_000;

/**
 * For a page that can't do anything without Docker — Docker, Databases and the
 * Data browser. Without it, their first `docker` call failed and the page showed
 * Docker's own error ("failed to connect to the docker API at
 * npipe:////./pipe/dockerDesktopLinuxEngine…") as "Something went wrong".
 * Asking first gets a specific message, and a button that does something
 * about it: Install from Apps when Docker is missing, Start when it's stopped.
 *
 * Hand `usable` to the page's own resources as `enabled`, so they wait for
 * Docker, and pass `status(resource)` to `<Page status>`.
 */
export function useDockerGate(): {
  detection: Resource<DockerDetection>;
  usable: boolean;
  status: <T>(resource: Resource<T>) => PageStatus;
} {
  const navigate = useNavigate();
  const detection = useResource(detectDocker);
  const usable = Boolean(detection.data?.found && detection.data.daemon_reachable);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);

  const start = useCallback(async () => {
    setStartError(null);
    setStarting(true);
    try {
      await startDockerEngine();
    } catch (e) {
      setStartError(e instanceof Error ? e.message : String(e));
      setStarting(false);
    }
  }, []);

  // Docker answers some seconds after it's asked to start, so keep asking
  // until it does — or give up and say so, rather than spin forever.
  useEffect(() => {
    if (!starting) return;
    const deadline = Date.now() + START_TIMEOUT_MS;
    const id = setInterval(() => {
      if (Date.now() > deadline) {
        setStarting(false);
        setStartError("Docker didn't answer within two minutes. Check Docker Desktop, then recheck.");
        return;
      }
      void detection.refresh();
    }, START_POLL_MS);
    return () => clearInterval(id);
  }, [starting, detection.refresh]);

  useEffect(() => {
    if (usable) setStarting(false);
  }, [usable]);

  const blocked = dockerBlockedStatus(detection.data, {
    onRecheck: () => void detection.refresh(),
    onStart: () => void start(),
    starting,
    startError,
    onOpenApps: () => navigate("/apps"),
  });
  return {
    detection,
    usable,
    status: (resource) => blocked ?? (usable ? statusOf(resource) : statusOf(detection)),
  };
}
