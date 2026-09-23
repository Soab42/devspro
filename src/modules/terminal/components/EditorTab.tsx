import { KeyboardEvent, useEffect, useState } from "react";
import { Button, Callout, Spinner } from "../../../ui";
import { CodeEditor } from "../../../ui/CodeEditor";
import { Icon } from "../../../ui/Icon";
import { File, Save, Server, House } from "../../../ui/icons";
import { fmReadFile, fmWriteFile } from "../../file-manager/api";
import { sftpReadFile, sftpWriteFile } from "../../ssh/api";
import { baseName, errorMessage } from "../../ssh/utils";
import type { EditorFile } from "../workspace";

/**
 * A file open in a tab of the terminal workspace, next to the shells. Local
 * files go through the file manager's commands, remote ones through the
 * sidebar's SFTP session. Ctrl/Cmd+S saves; the tab's dirty mark is the
 * workspace's, reported through `onDirty`.
 */
export default function EditorTab({
  file,
  active,
  onDirty,
  onSaved,
}: {
  file: EditorFile;
  active: boolean;
  onDirty: (dirty: boolean) => void;
  onSaved?: (message: string) => void;
}) {
  const [original, setOriginal] = useState<string | null>(null);
  const [contents, setContents] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const text = file.sftp ? await sftpReadFile(file.sftp, file.path) : await fmReadFile(file.path);
        if (cancelled) return;
        setOriginal(text);
        setContents(text);
      } catch (e) {
        if (!cancelled) setError(errorMessage(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [file.path, file.sftp]);

  const dirty = original !== null && contents !== original;
  useEffect(() => onDirty(dirty), [dirty, onDirty]);

  async function save() {
    if (!dirty || saving) return;
    setSaving(true);
    setError(null);
    try {
      const message = file.sftp
        ? await sftpWriteFile(file.sftp, file.path, contents)
        : await fmWriteFile(file.path, contents);
      setOriginal(contents);
      setNotice(message);
      onSaved?.(message);
      window.setTimeout(() => setNotice(null), 2500);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    }
  }

  const ext = file.path.split(".").pop()?.toLowerCase() ?? "";
  const language = ext === "yml" || ext === "yaml" ? "yaml" : ext === "sql" ? "sql" : undefined;

  return (
    <div className={`term-editor${active ? "" : " term-hidden"}`} onKeyDown={onKeyDown}>
      <div className="term-editor-bar">
        <Icon icon={file.sftp ? Server : House} size="sm" />
        <span className="term-editor-path" title={file.path}>
          {file.hostName ? `${file.hostName}: ` : ""}
          {file.path}
        </span>
        {notice && <span className="term-editor-notice">{notice}</span>}
        <Button variant={dirty ? "primary" : "secondary"} size="sm" icon={Save} loading={saving} disabled={!dirty} onClick={() => void save()}>
          Save
        </Button>
      </div>
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      {loading ? (
        <div className="term-editor-loading">
          <Spinner size="md" />
        </div>
      ) : original === null ? (
        <div className="term-editor-loading">
          <Icon icon={File} size="md" />
        </div>
      ) : (
        <CodeEditor value={contents} onChange={setContents} language={language} fill ariaLabel={`Edit ${baseName(file.path)}`} onSubmit={() => void save()} />
      )}
    </div>
  );
}
