import { useEffect, useMemo, useState } from "react";
import { unifiedMergeView } from "@codemirror/merge";
import { Callout, CopyButton, EmptyState, Icon, Modal, Spinner } from "../../../ui";
/* Not from the ui barrel, on purpose — see the note there. */
import { CodeEditor } from "../../../ui/CodeEditor";
import { GitCommitHorizontal, History } from "../../../ui/icons";
import { GitLogEntry, fmGitLog, fmGitShow } from "../api";
import { errorMessage, relativeTime } from "../utils";

/**
 * GitLens' file history: every commit that touched this file (through
 * renames), newest first, and what each one changed in it, shown inline
 * with the lines it removed.
 */
export default function GitHistory({
  path,
  name,
  initialSha,
  onClose,
}: {
  path: string;
  name: string;
  /** Opens on this commit, e.g. the one the cursor's line was blamed on. */
  initialSha?: string;
  onClose: () => void;
}) {
  const [log, setLog] = useState<GitLogEntry[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [diff, setDiff] = useState<{ sha: string; before: string; after: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fmGitLog(path)
      .then((entries) => {
        if (cancelled) return;
        setLog(entries);
        const start = entries.find((e) => initialSha && e.sha.startsWith(initialSha)) ?? entries[0];
        setSelected(start?.sha ?? null);
      })
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [path, initialSha]);

  const entry = log?.find((e) => e.sha === selected) ?? null;
  useEffect(() => {
    if (!entry) return;
    let cancelled = false;
    setDiff(null);
    fmGitShow(path, entry.sha, entry.path)
      .then((d) => !cancelled && setDiff({ sha: entry.sha, ...d }))
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [path, entry]);

  const extensions = useMemo(
    () => (diff ? unifiedMergeView({ original: diff.before, mergeControls: false, collapseUnchanged: { margin: 3, minSize: 6 } }) : []),
    [diff],
  );

  return (
    <Modal title={`History · ${name}`} icon={History} size="full" onClose={onClose}>
      {error && <Callout tone="danger">{error}</Callout>}
      {log === null && !error ? (
        <div className="fm-viewer-empty">
          <Spinner size="md" />
        </div>
      ) : log && log.length === 0 ? (
        <EmptyState icon={History} title="No history" description="This file has no commits yet." size="md" />
      ) : (
        <div className="fm-history">
          <ul className="fm-history-list" aria-label="Commits">
            {log?.map((e) => (
              <li key={e.sha}>
                <button
                  type="button"
                  className={e.sha === selected ? "fm-history-item fm-history-item-active" : "fm-history-item"}
                  onClick={() => setSelected(e.sha)}
                  title={`${e.summary}\n${e.author} <${e.email}>\n${new Date(e.time * 1000).toLocaleString()}`}
                >
                  <span className="fm-history-summary">{e.summary}</span>
                  <span className="fm-history-meta u-secondary">
                    {e.author} · {relativeTime(e.time)} · <span className="fm-history-sha">{e.sha.slice(0, 7)}</span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
          <div className="fm-history-diff">
            {entry && (
              <div className="fm-viewer-bar">
                <Icon icon={GitCommitHorizontal} size="xs" className="u-secondary" />
                <span className="fm-viewer-path" title={entry.summary}>
                  {entry.sha.slice(0, 7)} · {entry.summary}
                </span>
                <span className="fm-viewer-notice u-secondary">
                  {entry.author}, {new Date(entry.time * 1000).toLocaleString()}
                </span>
                <CopyButton value={entry.sha} label="Copy commit hash" size="sm" />
              </div>
            )}
            {diff && diff.sha === selected ? (
              <CodeEditor key={diff.sha} value={diff.after} onChange={() => undefined} readOnly fileName={entry?.path ?? name} fill ide extensions={extensions} ariaLabel={`${name} at ${diff.sha.slice(0, 7)}`} />
            ) : (
              <div className="fm-viewer-empty">
                <Spinner size="md" />
              </div>
            )}
          </div>
        </div>
      )}
    </Modal>
  );
}
