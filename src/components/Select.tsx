import { useEffect, useRef, useState } from "react";
import "./Select.css";

export interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps {
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  id?: string;
}

// A from-scratch dropdown, not a native <select> — WebKitGTK renders native
// <select> popups with the OS GTK theme's own colors, ignoring our CSS custom
// properties, which can make the option list unreadable in dark mode. Owning
// every pixel here guarantees it always matches the app's theme.
export default function Select({ value, onChange, options, id }: SelectProps) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleOutsideClick(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleOutsideClick);
    return () => document.removeEventListener("mousedown", handleOutsideClick);
  }, []);

  const current = options.find((o) => o.value === value);

  return (
    <div className="custom-select" ref={containerRef} id={id}>
      <button type="button" className="custom-select-trigger" onClick={() => setOpen((o) => !o)}>
        <span>{current?.label ?? value}</span>
        <span className="custom-select-arrow">▾</span>
      </button>
      {open && (
        <ul className="custom-select-menu" role="listbox">
          {options.map((o) => (
            <li
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              className={`custom-select-option${o.value === value ? " active" : ""}`}
              onClick={() => {
                onChange(o.value);
                setOpen(false);
              }}
            >
              {o.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
