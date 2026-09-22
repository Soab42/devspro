import { useCallback } from "react";
import { LogViewerModal } from "../../shared/LogViewerModal";
import { ScheduledTaskView, fetchScheduledTaskLogs } from "../api";

export default function LogsModal({
  task,
  onClose,
}: {
  task: ScheduledTaskView;
  onClose: () => void;
}) {
  const load = useCallback(
    (lines: number) => fetchScheduledTaskLogs(task.id, lines),
    [task.id],
  );

  return (
    <LogViewerModal
      title={`Output — ${task.name}`}
      fetchLogs={load}
      onClose={onClose}
      intervalMs={2000}
    />
  );
}
