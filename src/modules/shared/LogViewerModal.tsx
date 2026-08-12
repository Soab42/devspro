import { useEffect, useState } from "react";
import { Callout, CodeBlock, CopyButton, Modal, ModalFooter, Button, Stack } from "../../ui";
import { ScrollText } from "../../ui/icons";

export interface LogViewerModalProps {
  title: string;
  /** Called on mount and then every `intervalMs`. */
  fetchLogs: (lines: number) => Promise<string>;
  onClose: () => void;
  lines?: number;
  intervalMs?: number;
}

/**
 * Shared by the Docker and Workers log viewers, which were byte-for-byte
 * identical apart from their fetcher and poll interval.
 */
export function LogViewerModal({
  title,
  fetchLogs,
  onClose,
  lines = 300,
  intervalMs = 3000,
}: LogViewerModalProps) {
  const [logs, setLogs] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    async function load() {
      try {
        const text = await fetchLogs(lines);
        if (!active) return;
        setLogs(text);
        setError(null);
      } catch (e) {
        if (active) setError(e instanceof Error ? e.message : String(e));
      }
    }

    void load();
    const interval = setInterval(load, intervalMs);
    return () => {
      active = false;
      clearInterval(interval);
    };
  }, [fetchLogs, lines, intervalMs]);

  return (
    <Modal
      size="full"
      title={title}
      icon={ScrollText}
      description={`Last ${lines} lines, refreshing every ${Math.round(intervalMs / 1000)}s.`}
      onClose={onClose}
      footer={
        <ModalFooter start={<CopyButton value={logs} label="Copy logs" size="md" />}>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {/* A failed poll keeps the last good output on screen rather than
            replacing it — stale logs still beat an empty panel. */}
        {error && <Callout tone="danger">{error}</Callout>}
        <CodeBlock maxHeight="100%" autoScrollBottom>
          {logs || "(no output yet)"}
        </CodeBlock>
      </Stack>
    </Modal>
  );
}
