import { useCallback } from "react";
import { LogViewerModal } from "../../shared/LogViewerModal";
import { WorkerView, fetchWorkerLogs } from "../api";

export default function LogsModal({
  worker,
  onClose,
}: {
  worker: WorkerView;
  onClose: () => void;
}) {
  const load = useCallback(
    (lines: number) => fetchWorkerLogs(worker.id, lines),
    [worker.id],
  );

  return (
    <LogViewerModal
      title={`Logs — ${worker.name}`}
      fetchLogs={load}
      onClose={onClose}
      intervalMs={2000}
      laravelTable
      workingDir={worker.working_dir}
    />
  );
}
