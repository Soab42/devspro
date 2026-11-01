import { Callout, IconButton, Menu, SearchInput, SegmentedControl, Spinner } from "../../../ui";
import { Play, Plus, X } from "../../../ui/icons";
import { useCommandLibrary } from "../useCommandLibrary";
import { CommandList } from "./CommandList";
import { useCommandRunner } from "./useCommandRunner";
import { SCOPE_OPTIONS, copyText, useLibraryView } from "./useLibraryView";
import "./Commands.css";

/**
 * The command library beside the terminal. A click types the command into
 * the focused pane for the user to check and send; the play button runs it.
 */
export default function CommandsPanel({
  /** Writes into the focused pane; null when no terminal has focus. */
  write,
  onClose,
  onError,
}: {
  write: ((text: string) => void) | null;
  onClose: () => void;
  onError: (message: string) => void;
}) {
  const library = useCommandLibrary();
  const view = useLibraryView(library, onError);
  const runner = useCommandRunner({
    insert: write ?? undefined,
    run: write ? (text) => write(`${text}\r`) : undefined,
    copy: (text) => void copyText(text).catch((e) => onError(String(e))),
  });

  return (
    <div className="cmd-panel">
      <div className="cmd-panel-head">
        <span className="cmd-panel-title">Commands</span>
        <IconButton icon={Plus} label="Add a command" size="sm" onClick={view.addNew} />
        <IconButton icon={X} label="Hide commands" size="sm" onClick={onClose} />
      </div>
      <div className="cmd-panel-filters">
        <SearchInput value={view.query} onChange={view.setQuery} placeholder="Search commands…" grow={false} />
        <SegmentedControl ariaLabel="Which commands" size="sm" value={view.scope} onChange={view.setScope} options={SCOPE_OPTIONS} />
      </div>
      <div className="cmd-panel-body">
        {library.error ? (
          <Callout tone="danger">{library.error}</Callout>
        ) : library.loading ? (
          <Spinner size="sm" />
        ) : view.matches === 0 ? (
          <p className="cmd-panel-hint">
            {view.scope === "personal" && !view.query ? "No personal commands yet. Add one with +." : "Nothing matches."}
          </p>
        ) : (
          <CommandList
            sections={view.sections}
            searching={view.query.trim() !== ""}
            compact
            storageKey="devspro.commands.panel.open"
            pickLabel={write ? "Type it in" : "Copy"}
            onPick={(c) => runner.start(c, write ? "insert" : "copy")}
            actions={(c) => (
              <>
                {write && <IconButton icon={Play} label="Run" size="sm" onClick={() => runner.start(c, "run")} />}
                <Menu items={view.menuItems(c)} label="More" size="sm" align="end" />
              </>
            )}
          />
        )}
      </div>
      <div className="cmd-panel-hint">
        {write ? "Click a command to type it in; the play button runs it." : "Focus a terminal to type commands into it. Clicking copies for now."}
      </div>
      {runner.element}
      {view.formElement}
    </div>
  );
}
