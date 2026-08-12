import type { PageStatus } from "../../ui";
import { Mono } from "../../ui";
import { PackageX, PlugZap } from "../../ui/icons";
import type { DockerDetection } from "./api";

/**
 * Docker's two precondition failures, as a PageStatus.
 *
 * Shared because the Docker and Mail Catcher pages both gate on exactly the
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
          Install it from{" "}
          <a
            href="https://docs.docker.com/engine/install/"
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
          <p>
            If that mentions permission denied, make sure your user is in the{" "}
            <Mono variant="chip">docker</Mono> group — run{" "}
            <Mono variant="chip">sudo usermod -aG docker $USER</Mono>, then log out and
            back in. DevsPro never runs Docker commands elevated.
          </p>
        </>
      ),
      action: { label: "Recheck", onClick: onRecheck },
    };
  }

  return null;
}
