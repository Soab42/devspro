import { useEffect, useState } from "react";
import { Button, Callout, Modal, ModalFooter, Mono, Spinner, Stack } from "../../../ui";
import { Info } from "../../../ui/icons";
import Thumbnail from "./Thumbnail";
import { DirStats, FileDetail, FileEntry, fmDirStats, fmStatDetail } from "../api";
import { errorMessage, fileKind, formatModified, formatSize, ownerLabel, permissionsLabel, tildePath } from "../utils";

/**
 * "Get Info" for one entry, or a headcount for several. Folders also get a
 * walked size, which is the one figure a listing can never show.
 */
export default function PropertiesModal({ entries, home, onClose }: { entries: FileEntry[]; home: string; onClose: () => void }) {
  const single = entries.length === 1 ? entries[0] : null;
  const [detail, setDetail] = useState<FileDetail | null>(null);
  const [stats, setStats] = useState<DirStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!single) return;
    let cancelled = false;
    setDetail(null);
    setStats(null);
    setError(null);
    fmStatDetail(single.path)
      .then((d) => !cancelled && setDetail(d))
      .catch((e) => !cancelled && setError(errorMessage(e)));
    if (single.is_dir) {
      fmDirStats(single.path)
        .then((s) => !cancelled && setStats(s))
        .catch((e) => !cancelled && setError(errorMessage(e)));
    }
    return () => {
      cancelled = true;
    };
  }, [single]);

  const title = single ? `${single.name} properties` : `${entries.length} items`;
  const files = entries.filter((e) => !e.is_dir);
  const folders = entries.filter((e) => e.is_dir);

  return (
    <Modal
      size="sm"
      title={title}
      icon={Info}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {single ? (
          <>
            <div className="fm-preview-hero">
              <Thumbnail entry={single} size={256} className="fm-preview-icon" />
              <span className="fm-preview-name" title={single.name}>
                {single.name}
              </span>
              <span className="fm-preview-kind u-secondary">{fileKind(single)}</span>
            </div>
            <dl className="fm-preview-meta">
              <dt>Path</dt>
              <dd>
                <Mono truncate copyable copyValue={single.path} title={single.path}>
                  {tildePath(single.path, home)}
                </Mono>
              </dd>
              <dt>Type</dt>
              <dd>{detail ? <Mono>{detail.mime}</Mono> : fileKind(single)}</dd>
              <dt>Size</dt>
              <dd>
                {single.is_dir ? (
                  stats ? (
                    `${stats.truncated ? "over " : ""}${formatSize(stats.bytes)} · ${stats.files} files, ${stats.dirs} folders`
                  ) : (
                    <Spinner />
                  )
                ) : (
                  `${formatSize(single.size)} (${single.size.toLocaleString()} bytes)`
                )}
              </dd>
              <dt>Modified</dt>
              <dd>{formatModified(single.modified)}</dd>
              <dt>Created</dt>
              <dd>{detail ? formatModified(detail.created) : <Spinner />}</dd>
              <dt>Permissions</dt>
              <dd>
                <Mono>{permissionsLabel(single.path, single.permissions, detail?.mode_symbolic)}</Mono>
              </dd>
              <dt>Owner</dt>
              <dd>{detail ? <Mono>{ownerLabel(detail.owner, detail.group)}</Mono> : <Spinner />}</dd>
              {single.is_symlink && (
                <>
                  <dt>Link</dt>
                  <dd>Symbolic link</dd>
                </>
              )}
            </dl>
          </>
        ) : (
          <dl className="fm-preview-meta">
            <dt>Selected</dt>
            <dd>
              {folders.length} {folders.length === 1 ? "folder" : "folders"}, {files.length} {files.length === 1 ? "file" : "files"}
            </dd>
            <dt>Files total</dt>
            <dd>{formatSize(files.reduce((n, e) => n + e.size, 0))}</dd>
          </dl>
        )}
        {error && <Callout tone="danger">{error}</Callout>}
      </Stack>
    </Modal>
  );
}
