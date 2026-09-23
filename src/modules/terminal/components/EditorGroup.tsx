import { Icon } from "../../../ui/Icon";
import { File, X } from "../../../ui/icons";
import { baseName } from "../../ssh/utils";
import { editorKey, type EditorWindow } from "../workspace";
import EditorTab from "./EditorTab";

/**
 * The editor tab of the workspace: every open file as an inner tab, the
 * active one shown below, the same shape as an IDE's editor group. Every
 * file stays mounted while hidden so unsaved edits survive switching.
 */
export default function EditorGroup({
  window: win,
  hidden,
  onActivate,
  onClose,
  onDirty,
}: {
  window: EditorWindow;
  hidden: boolean;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
  onDirty: (key: string, dirty: boolean) => void;
}) {
  return (
    <div className={`term-editor-group${hidden ? " term-hidden" : ""}`}>
      <div className="term-editor-tabs" role="tablist" aria-label="Open files">
        {win.files.map((file) => {
          const key = editorKey(file);
          const active = key === win.active;
          const dirty = win.dirty[key] ?? false;
          return (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={active}
              className={`term-editor-tab${active ? " term-editor-tab-active" : ""}${dirty ? " term-editor-tab-dirty" : ""}`}
              title={file.hostName ? `${file.hostName}: ${file.path}` : file.path}
              onClick={() => onActivate(key)}
              onAuxClick={(e) => {
                if (e.button === 1) onClose(key);
              }}
            >
              <Icon icon={File} size="xs" />
              <span className="term-editor-tab-name">
                {dirty ? "● " : ""}
                {baseName(file.path)}
              </span>
              <span
                role="button"
                tabIndex={-1}
                aria-label={`Close ${baseName(file.path)}`}
                className="term-window-close"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(key);
                }}
              >
                <Icon icon={X} size="xs" />
              </span>
            </button>
          );
        })}
      </div>
      {win.files.map((file) => {
        const key = editorKey(file);
        return <EditorTab key={key} file={file} active={!hidden && key === win.active} onDirty={(d) => onDirty(key, d)} />;
      })}
    </div>
  );
}
