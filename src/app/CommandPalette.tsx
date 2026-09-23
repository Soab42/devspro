import { KeyboardEvent, MouseEvent, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { EmptyState, Icon } from "../ui";
import { Search, SearchX } from "../ui/icons";
import { modules as allModules, type ModuleDefinition } from "./ModuleRegistry";
import { useModules } from "./ModulesContext";
import { openModuleWindow } from "./popout";
import { rankModules } from "./moduleSearch";
import "./CommandPalette.css";

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
}

interface PaletteOption {
  module: ModuleDefinition;
  /** Switched off — activating it enables it first. */
  disabled: boolean;
}

const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

/**
 * The global search launcher.
 *
 * Always mounted; `open` only decides whether it renders. Toggling a prop
 * rather than mounting/unmounting keeps the reset-on-open and
 * restore-focus-on-close effects running on the SAME component instance
 * across repeated opens, which is what lets the "give focus back to whatever
 * had it" behaviour work at all.
 *
 * Ranks the enabled modules and the switched-off ones separately — see
 * moduleSearch.ts for the scoring — and lists them as two groups so a
 * switched-off match never outranks an already-open module for the same
 * query.
 */
export function CommandPalette({ open, onClose }: CommandPaletteProps) {
  const navigate = useNavigate();
  const { enabled, disabled: disabledIds, setEnabled } = useModules();
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  const disabledModules = useMemo(
    () => allModules.filter((m) => disabledIds.has(m.id)),
    [disabledIds],
  );

  const enabledMatches = useMemo(() => rankModules(query, enabled), [query, enabled]);
  const disabledMatches = useMemo(
    () => rankModules(query, disabledModules),
    [query, disabledModules],
  );

  const options: PaletteOption[] = useMemo(
    () => [
      ...enabledMatches.map((module) => ({ module, disabled: false })),
      ...disabledMatches.map((module) => ({ module, disabled: true })),
    ],
    [enabledMatches, disabledMatches],
  );

  // A fresh search every time the palette is summoned. useLayoutEffect, not
  // useEffect, so the reset lands before the browser paints — otherwise the
  // previous query would flash for a frame.
  useLayoutEffect(() => {
    if (open) {
      setQuery("");
      setHighlight(0);
    }
  }, [open]);

  // Re-typing narrows the list out from under whatever was highlighted;
  // always land back on the top match.
  useEffect(() => {
    setHighlight(0);
  }, [query]);

  // Focus in on open; give focus back to whatever had it once the palette closes.
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    inputRef.current?.focus();
    return () => previous?.focus?.();
  }, [open]);

  if (!open) return null;

  // Picking a module opens it fresh in its own window: the launcher is the
  // "give me a new one" gesture, while the top-bar button carries the page
  // you're on with it. Ctrl/Cmd switches to opening in this window instead.
  async function activate(option: PaletteOption | undefined, inPlace: boolean) {
    if (!option) return;
    if (option.disabled) await setEnabled(option.module.id, true);
    if (inPlace) {
      navigate(option.module.path);
    } else {
      await openModuleWindow(option.module);
    }
    onClose();
  }

  function onInputKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        setHighlight((i) => (options.length === 0 ? 0 : (i + 1) % options.length));
        break;
      case "ArrowUp":
        e.preventDefault();
        setHighlight((i) => (options.length === 0 ? 0 : (i - 1 + options.length) % options.length));
        break;
      case "Enter":
        e.preventDefault();
        void activate(options[highlight], e.ctrlKey || e.metaKey);
        break;
      case "Escape":
        e.preventDefault();
        onClose();
        break;
    }
  }

  // mousedown, not click: a click that STARTS inside the panel and ends on
  // the overlay (dragging while selecting text) must not close the palette.
  function onOverlayMouseDown(e: MouseEvent<HTMLDivElement>) {
    if (e.target === e.currentTarget) onClose();
  }

  function optionId(index: number): string {
    return `${listboxId}-option-${index}`;
  }

  function renderGroup(heading: string, matches: ModuleDefinition[], startIndex: number, hint?: string) {
    if (matches.length === 0) return null;
    return (
      <div className="palette-group" role="group" aria-label={heading}>
        <p className="palette-group-label" aria-hidden="true">
          {heading}
        </p>
        {matches.map((m, i) => {
          const index = startIndex + i;
          return (
            <div
              key={m.id}
              id={optionId(index)}
              role="option"
              aria-selected={index === highlight}
              className={`palette-option${index === highlight ? " palette-option-active" : ""}`}
              onMouseEnter={() => setHighlight(index)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={(e) => void activate(options[index], e.ctrlKey || e.metaKey)}
            >
              <Icon icon={m.icon} size="sm" className="palette-option-icon" />
              <span className="palette-option-label">{m.label}</span>
              {m.description && <span className="palette-option-desc">{m.description}</span>}
              {hint && <span className="palette-option-hint">{hint}</span>}
            </div>
          );
        })}
      </div>
    );
  }

  const activeOption = options[highlight];

  return (
    <div className="palette-overlay" onMouseDown={onOverlayMouseDown}>
      <div className="palette-panel" role="dialog" aria-modal="true" aria-label="Search modules">
        <div className="palette-search">
          <Icon icon={Search} size="sm" className="palette-search-icon" />
          <input
            ref={inputRef}
            type="text"
            className="palette-input"
            role="combobox"
            aria-expanded="true"
            aria-controls={listboxId}
            aria-autocomplete="list"
            aria-activedescendant={activeOption ? optionId(highlight) : undefined}
            autoFocus
            placeholder="Search modules…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onInputKeyDown}
          />
        </div>

        <div className="palette-list" id={listboxId} role="listbox" aria-label="Modules">
          {renderGroup("Open", enabledMatches, 0)}
          {renderGroup("Switched off", disabledMatches, enabledMatches.length, "Enable and open")}
          {options.length === 0 && (
            <EmptyState icon={SearchX} title={`No module matches “${query}”`} size="sm" />
          )}
        </div>

        <div className="palette-footer">
          <span className="palette-hint">
            <kbd>↑↓</kbd> navigate
          </span>
          <span className="palette-hint">
            <kbd>↵</kbd> new window
          </span>
          <span className="palette-hint">
            <kbd>{isMac ? "⌘" : "ctrl"} ↵</kbd> open here
          </span>
          <span className="palette-hint">
            <kbd>esc</kbd> close
          </span>
        </div>
      </div>
    </div>
  );
}
