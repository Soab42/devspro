import { useEffect, useState } from "react";
import { Button, Callout } from "../../../../ui";
import { PlugZap } from "../../../../ui/icons";
import { detectDocker } from "../../../docker/api";
import type { DockerDetection } from "../../../docker/api";
import { dockerBlockedStatus } from "../../../docker/blocked";

/**
 * The Callout the Docker page shows when Docker is missing or its daemon is
 * unreachable, built from the same `dockerBlockedStatus` so the guidance
 * (including the docker-group advice) stays in one place. `message` is the
 * scan's own reason, used if detection can't say more.
 */
export default function UnavailableCallout({ message, onRecheck }: { message: string | null; onRecheck: () => void }) {
  const [detection, setDetection] = useState<DockerDetection | null>(null);

  useEffect(() => {
    let live = true;
    detectDocker().then(
      (d) => live && setDetection(d),
      () => live && setDetection(null),
    );
    return () => {
      live = false;
    };
  }, [message]);

  const status = dockerBlockedStatus(detection, onRecheck);
  if (status?.kind === "blocked") {
    return (
      <Callout
        tone="warning"
        icon={status.icon}
        title={status.title}
        actions={status.action && <Button onClick={status.action.onClick}>{status.action.label}</Button>}
      >
        {status.message}
      </Callout>
    );
  }
  return (
    <Callout
      tone="warning"
      icon={PlugZap}
      title="Docker isn't available"
      actions={<Button onClick={onRecheck}>Recheck</Button>}
    >
      {message ?? "Docker isn't installed or its daemon isn't running."}
    </Callout>
  );
}
