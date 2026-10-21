import { useEffect, useState } from "react";
import { Button, CopyButton, Mono, Spinner } from "../../../ui";
import { ExternalLink, Eye, HardDrive } from "../../../ui/icons";
import Thumbnail from "./Thumbnail";
import { FileDetail, FileEntry, fmStatDetail } from "../api";
import { errorMessage, fileKind, formatModified, formatSize, ownerLabel, permissionsLabel } from "../utils";

/**
 * The rightmost column once a file is selected: a big icon, the facts a
 * listing does not carry (owner, creation time, MIME, symbolic mode) and the
 * verbs that act on exactly this file.
 */
export default function FilePreview({
  entry,
  shownPath,
  onOpen,
  onReveal,
}: {
  entry: FileEntry;
  /** The path as people read it: `~/…`, or from a device's name. */
  shownPath: string;
  /** In a tab, or in the browser for a Google Doc. */
  onOpen: (entry: FileEntry) => void;
  onReveal: (entry: FileEntry) => void;
}) {
  const [detail, setDetail] = useState<FileDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
    fmStatDetail(entry.path)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch((e) => {
        if (!cancelled) setError(errorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [entry.path]);

  return (
    <div className="fm-preview" aria-label={`Preview of ${entry.name}`}>
      <div className="fm-preview-hero">
        <Thumbnail entry={entry} size={256} className="fm-preview-icon" />
        <span className="fm-preview-name" title={entry.name}>
          {entry.name}
        </span>
        <span className="fm-preview-kind u-secondary">
          {fileKind(entry)} · {formatSize(entry.size)}
        </span>
      </div>

      <div className="fm-preview-actions">
        <Button variant="primary" size="sm" icon={entry.link ? ExternalLink : Eye} onClick={() => onOpen(entry)}>
          {entry.link ? "Open in browser" : "Open"}
        </Button>
        <Button variant="secondary" size="sm" icon={HardDrive} onClick={() => onReveal(entry)}>
          Reveal
        </Button>
        <CopyButton value={entry.path} label="Copy path" size="sm" />
      </div>

      <dl className="fm-preview-meta">
        <dt>Path</dt>
        <dd>
          <Mono truncate title={entry.path}>
            {shownPath}
          </Mono>
        </dd>
        <dt>Type</dt>
        <dd>{detail ? <Mono>{detail.mime}</Mono> : fileKind(entry)}</dd>
        <dt>Size</dt>
        <dd>{formatSize(entry.size)}</dd>
        <dt>Modified</dt>
        <dd>{formatModified(entry.modified)}</dd>
        <dt>Created</dt>
        <dd>{detail ? formatModified(detail.created) : <Spinner />}</dd>
        <dt>Permissions</dt>
        <dd>
          <Mono>{permissionsLabel(entry.path, entry.permissions, detail?.mode_symbolic)}</Mono>
        </dd>
        <dt>Owner</dt>
        <dd>{detail ? <Mono>{ownerLabel(detail.owner, detail.group)}</Mono> : <Spinner />}</dd>
      </dl>
      {error && <p className="fm-col-note u-secondary">{error}</p>}
    </div>
  );
}
