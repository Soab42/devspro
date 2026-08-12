import { useCallback } from "react";
import { LogViewerModal } from "../../shared/LogViewerModal";
import { Container, fetchLogs } from "../api";

export default function LogsModal({
  container,
  onClose,
}: {
  container: Container;
  onClose: () => void;
}) {
  const load = useCallback(
    (lines: number) => fetchLogs(container.id, lines),
    [container.id],
  );

  return (
    <LogViewerModal
      title={`Logs — ${container.name}`}
      fetchLogs={load}
      onClose={onClose}
      intervalMs={3000}
    />
  );
}
