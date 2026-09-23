import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown, Search } from "./icons";
import { Icon } from "./Icon";
import "./Select.css";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  id?: string;
  disabled?: boolean;
  placeholder?: string;
  size?: "sm" | "md";
  /**
   * Filters the option list from a box at the top of the menu. Worth turning
   * on wherever the list is long enough that scrolling it is the slow way to
   * find something — a machine's Docker containers, say.
   */
  searchable?: boolean;
  searchPlaceholder?: string;
  "aria-label"?: string;
  className?: string;
  /**
   * A menu wider than its trigger, in px, for lists whose labels run longer
   * than the selected value — device presets, say. Kept on screen by
   * right-aligning when there is no room to the right.
   */
  menuWidth?: number;
  /** Fires as the menu opens and closes. The Browser module hides its native page views while any popup is up. */
  onOpenChange?: (open: boolean) => void;
}

// A from-scratch dropdown, not a native <select> — WebKitGTK renders native
// <select> popups with the OS GTK theme's own colors, ignoring our CSS custom
// properties, which can make the option list unreadable in dark mode. Owning
// every pixel here guarantees it always matches the app's theme.
//
// Do not "simplify" this back to a native <select>, and do not introduce one
// elsewhere — there is a guard test asserting the app contains no <select>.
export function Select({
  value,
  onChange,
  options,
  id,
  disabled,
  placeholder,
  size = "md",
  searchable = false,
  searchPlaceholder = "Search…",
  "aria-label": ariaLabel,
  className,
  onOpenChange,
  menuWidth,
}: SelectProps) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    onOpenChange?.(open);
    // Report only on change; a new callback identity is not a change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [query, setQuery] = useState("");
  const [position, setPosition] = useState<{
    top: number;
    left: number;
    width: number;
  } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  // What the menu actually shows. `activeIndex` indexes *this*, not `options`,
  // so arrow keys walk what is on screen rather than through hidden entries.
  const visible = useMemo(() => {
    const q = searchable ? query.trim().toLowerCase() : "";
    if (!q) return options;
    return options.filter(
      (o) =>
        o.label.toLowerCase().includes(q) || o.value.toLowerCase().includes(q),
    );
  }, [options, query, searchable]);

  // The listbox renders into a portal because every place this control is used
  // sits inside a clipping context: .card has overflow:hidden for its rounded
  // corners, and .dt-scroll combines overflow-x:auto with container-type, which
  // contains even absolutely-positioned descendants. Anchored inline, the menu
  // was simply cut off at the card or table edge.
  //
  // Re-measured as the filter narrows the list, since that changes the height
  // the flip-above decision depends on.
  useLayoutEffect(() => {
    if (!open || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const height = menuRef.current?.offsetHeight ?? 0;
    const spaceBelow = window.innerHeight - rect.bottom;
    // Flip above the trigger when there isn't room beneath it.
    const top =
      spaceBelow < height + 8 && rect.top > height
        ? rect.top - height - 4
        : rect.bottom + 4;
    const width = Math.max(rect.width, menuWidth ?? 0);
    const left =
      rect.left + width > window.innerWidth - 8
        ? Math.max(8, rect.right - width)
        : rect.left;
    setPosition({ top, left, width });
  }, [open, visible.length, menuWidth]);

  // The search box takes focus on open, so typing filters immediately instead
  // of needing a click into the field first.
  useEffect(() => {
    if (open && searchable) searchRef.current?.focus();
  }, [open, searchable]);

  useEffect(() => {
    if (!open) return;

    function handleOutsideClick(e: MouseEvent) {
      const target = e.target as Node;
      if (
        containerRef.current?.contains(target) ||
        menuRef.current?.contains(target)
      ) {
        return;
      }
      setOpen(false);
    }
    // The menu is viewport-anchored, so it would visibly detach from its
    // trigger if the page moved under it.
    function close() {
      setOpen(false);
    }

    // ...but the listbox is itself scrollable (max-height + overflow-y), and
    // the listener below is in the capture phase, so its own scrolling reached
    // `close` too: a long option list shut the moment you tried to read past
    // the first few entries. Only an *ancestor* scrolling counts.
    function handleScroll(e: Event) {
      if (menuRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    }

    document.addEventListener("mousedown", handleOutsideClick);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", handleScroll, true);
    };
  }, [open]);

  const current = options.find((o) => o.value === value);

  function openMenu() {
    setQuery("");
    const selected = options.findIndex((o) => o.value === value);
    setActiveIndex(selected >= 0 ? selected : 0);
    setOpen(true);
  }

  function pick(option: SelectOption) {
    onChange(option.value);
    setOpen(false);
  }

  // The original had no keyboard handling at all — the menu was mouse-only,
  // which made it a dead end for keyboard users on every form in the app.
  // Shared by the trigger and the search box, so navigation works the same
  // whichever of the two holds focus.
  function onKeyDown(e: React.KeyboardEvent) {
    if (!open) {
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
        e.preventDefault();
        openMenu();
      }
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
        break;
      case "ArrowDown":
        e.preventDefault();
        if (visible.length) setActiveIndex((i) => (i + 1) % visible.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        if (visible.length)
          setActiveIndex((i) => (i - 1 + visible.length) % visible.length);
        break;
      case "Home":
        e.preventDefault();
        setActiveIndex(0);
        break;
      case "End":
        e.preventDefault();
        setActiveIndex(Math.max(0, visible.length - 1));
        break;
      case "Enter":
        e.preventDefault();
        if (visible[activeIndex]) pick(visible[activeIndex]);
        break;
      // Space picks the active option from the trigger, but inside the search
      // box it has to stay an ordinary character — a filter you cannot type a
      // space into is worse than no filter.
      case " ":
        if (searchable && e.target === searchRef.current) break;
        e.preventDefault();
        if (visible[activeIndex]) pick(visible[activeIndex]);
        break;
    }
  }

  const listboxId = `${id ?? "select"}-listbox`;
  const activeId = visible[activeIndex]
    ? `${id}-opt-${activeIndex}`
    : undefined;

  return (
    <div
      className={`custom-select custom-select-${size}${className ? ` ${className}` : ""}`}
      ref={containerRef}
    >
      <button
        type="button"
        id={id}
        ref={triggerRef}
        className="custom-select-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        onClick={() => (open ? setOpen(false) : openMenu())}
        onKeyDown={onKeyDown}
      >
        <span className="custom-select-value">
          {current?.label ?? placeholder ?? value}
        </span>
        <Icon icon={ChevronDown} size="xs" className="custom-select-arrow" />
      </button>
      {open &&
        createPortal(
          <div
            ref={menuRef}
            className="custom-select-popover"
            style={{
              top: position?.top ?? -9999,
              left: position?.left ?? -9999,
              width: position?.width,
              // Hidden for the first frame, before useLayoutEffect has
              // measured it — otherwise it flashes at the top-left corner.
              visibility: position ? "visible" : "hidden",
            }}
          >
            {searchable && (
              <div className="custom-select-search">
                <Icon
                  icon={Search}
                  size="sm"
                  className="custom-select-search-icon"
                />
                <input
                  ref={searchRef}
                  type="text"
                  className="custom-select-search-input"
                  value={query}
                  placeholder={searchPlaceholder}
                  spellCheck={false}
                  autoComplete="off"
                  role="combobox"
                  aria-expanded
                  aria-controls={listboxId}
                  aria-autocomplete="list"
                  aria-activedescendant={activeId}
                  aria-label={
                    ariaLabel ? `Filter ${ariaLabel}` : searchPlaceholder
                  }
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setActiveIndex(0);
                  }}
                  onKeyDown={onKeyDown}
                />
              </div>
            )}
            <ul
              id={listboxId}
              className="custom-select-menu"
              role="listbox"
              aria-activedescendant={searchable ? undefined : activeId}
            >
              {visible.map((option, index) => (
                <li
                  key={option.value}
                  id={`${id}-opt-${index}`}
                  role="option"
                  aria-selected={option.value === value}
                  className={[
                    "custom-select-option",
                    option.value === value && "custom-select-option-selected",
                    index === activeIndex && "custom-select-option-active",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  onMouseEnter={() => setActiveIndex(index)}
                  onClick={() => pick(option)}
                >
                  {option.label}
                </li>
              ))}
              {visible.length === 0 && (
                <li className="custom-select-empty" role="presentation">
                  No matches
                </li>
              )}
            </ul>
          </div>,
          document.body,
        )}
    </div>
  );
}
