import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "./icons";
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
  "aria-label"?: string;
  className?: string;
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
  "aria-label": ariaLabel,
  className,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [position, setPosition] = useState<{
    top: number;
    left: number;
    width: number;
  } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);

  // The listbox renders into a portal because every place this control is used
  // sits inside a clipping context: .card has overflow:hidden for its rounded
  // corners, and .dt-scroll combines overflow-x:auto with container-type, which
  // contains even absolutely-positioned descendants. Anchored inline, the menu
  // was simply cut off at the card or table edge.
  useLayoutEffect(() => {
    if (!open || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const height = menuRef.current?.offsetHeight ?? 0;
    const spaceBelow = window.innerHeight - rect.bottom;
    // Flip above the trigger when there isn't room beneath it.
    const top =
      spaceBelow < height + 8 && rect.top > height ? rect.top - height - 4 : rect.bottom + 4;
    setPosition({ top, left: rect.left, width: rect.width });
  }, [open, options.length]);

  useEffect(() => {
    if (!open) return;

    function handleOutsideClick(e: MouseEvent) {
      const target = e.target as Node;
      if (containerRef.current?.contains(target) || menuRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    // Viewport-anchored, so it would visibly detach from its trigger if either
    // of these happened while open.
    function close() {
      setOpen(false);
    }

    document.addEventListener("mousedown", handleOutsideClick);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [open]);

  const selectedIndex = options.findIndex((o) => o.value === value);
  const current = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  function openMenu() {
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : 0);
    setOpen(true);
  }

  // The original had no keyboard handling at all — the menu was mouse-only,
  // which made it a dead end for keyboard users on every form in the app.
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
        break;
      case "ArrowDown":
        e.preventDefault();
        setActiveIndex((i) => (i + 1) % options.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        setActiveIndex((i) => (i - 1 + options.length) % options.length);
        break;
      case "Home":
        e.preventDefault();
        setActiveIndex(0);
        break;
      case "End":
        e.preventDefault();
        setActiveIndex(options.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (options[activeIndex]) onChange(options[activeIndex].value);
        setOpen(false);
        break;
    }
  }

  return (
    <div
      className={`custom-select custom-select-${size}${className ? ` ${className}` : ""}`}
      ref={containerRef}
    >
      <button
        type="button"
        id={id}
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
          <ul
            ref={menuRef}
            className="custom-select-menu"
            role="listbox"
            aria-activedescendant={`${id}-opt-${activeIndex}`}
            style={{
              top: position?.top ?? -9999,
              left: position?.left ?? -9999,
              width: position?.width,
              // Hidden for the first frame, before useLayoutEffect has
              // measured it — otherwise it flashes at the top-left corner.
              visibility: position ? "visible" : "hidden",
            }}
          >
            {options.map((option, index) => (
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
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
            >
                {option.label}
              </li>
            ))}
          </ul>,
          document.body,
        )}
    </div>
  );
}
