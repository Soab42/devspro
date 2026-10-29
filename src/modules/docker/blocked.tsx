import type { PageStatus } from "../../ui";
import { Mono } from "../../ui";
import { PackageX, PlugZap } from "../../ui/icons";
import { isWindows } from "../../app/platform";
import type { DockerDetection } from "./api";

/**
 * Docker's two precondition failures, as a PageStatus.
 *
 * Shared because the Docker page and the Cleaner's Docker tab both gate on exactly the
 * same conditions and previously carried their own near-identical copies of
 * this markup — including the group-membership guidance below, which is
 * load-bearing security information and must not be dropped or reworded away.
 *
 * Returns null when Docker is usable, or when detection hasn't completed yet.
 */
export function dockerBlockedStatus(
  detection: DockerDetection | null,
  onRecheck: () => void,
): PageStatus | null {
  if (!detection) return null;

  if (!detection.found) {
    return {
      kind: "blocked",
      icon: PackageX,
      title: "Docker isn't installed",
      message: (
        <>
          Install {isWindows ? "Docker Desktop" : "it"} from{" "}
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
        </>
      ),
      action: { label: "Recheck", onClick: onRecheck },
    };
  }

  if (!detection.daemon_reachable) {
    return {
      kind: "blocked",
      icon: PlugZap,
      tone: "warning",
      title: "Docker daemon unreachable",
      message: (
        <>
          <p>{detection.error ?? "Could not reach the Docker daemon."}</p>
          {isWindows ? (
            // No docker group on Windows: the daemon lives inside Docker Desktop,
            // and it is unreachable because Docker Desktop isn't running yet.
            <p>
              Start Docker Desktop and wait until it reports the engine as running,
              then recheck. DevsPro never runs Docker commands elevated.
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
      action: { label: "Recheck", onClick: onRecheck },
    };
  }

  return null;
}
