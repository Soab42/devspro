import { ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy } from "./icons";
import { IconButton } from "./Button";
import "./Mono.css";

/* ----------------------------------------------------------------- Mono -- */

export interface MonoProps {
  children: ReactNode;
  /** "chip" adds the bordered inset treatment the old <code> element had. */
  variant?: "plain" | "chip";
  /** true → ellipsis at the container width; a number → max-width in px; a
   * string → any CSS length, so a cap can scale with the window. */
  truncate?: boolean | number | string;
  copyable?: boolean;
  /** Value placed on the clipboard. Defaults to the text of `children`. */
  copyValue?: string;
  title?: string;
  className?: string;
}

/**
 * Every technical value goes through here: ports, PIDs, hostnames, paths,
 * container IDs, image tags, IPs. It is the single strongest signal in the
 * design that a string is machine data rather than prose.
 */
export function Mono({
  children,
  variant = "plain",
  truncate,
  copyable = false,
  copyValue,
  title,
  className,
}: MonoProps) {
  const classes = [
    "mono",
    `mono-${variant}`,
    truncate && "mono-truncate",
    copyable && "mono-copyable",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  const value = copyValue ?? (typeof children === "string" ? children : undefined);

  return (
    <span
      className={classes}
      title={title ?? (typeof children === "string" ? children : undefined)}
      style={
        typeof truncate === "number" || typeof truncate === "string"
          ? { maxWidth: truncate }
          : undefined
      }
    >
      <span className="mono-value">{children}</span>
      {copyable && value !== undefined && <CopyButton value={value} />}
    </span>
  );
}

/* ----------------------------------------------------------- CopyButton -- */

export interface CopyButtonProps {
  value: string;
  label?: string;
  size?: "sm" | "md";
}

export function CopyButton({ value, label = "Copy", size = "sm" }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Without this the timeout fires into an unmounted component whenever a row
  // is removed (or the table refreshes) within the 1.5s confirmation window.
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard can be denied; failing silently is better than throwing here.
    }
  }, [value]);

  return (
    <IconButton
      icon={copied ? Check : Copy}
      label={copied ? "Copied" : label}
      size={size}
      variant="ghost"
      onClick={copy}
      className={copied ? "copy-button copy-button-done" : "copy-button"}
    />
  );
}

/* ------------------------------------------------------------ CodeBlock -- */

export interface CodeBlockProps {
  children: string;
  maxHeight?: string;
  /** Keeps the view pinned to the newest line — for log tails. */
  autoScrollBottom?: boolean;
  wrap?: boolean;
  className?: string;
}

export function CodeBlock({
  children,
  maxHeight = "50vh",
  autoScrollBottom = false,
  wrap = true,
  className,
}: CodeBlockProps) {
  const ref = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (!autoScrollBottom || !ref.current) return;
    ref.current.scrollTop = ref.current.scrollHeight;
  }, [children, autoScrollBottom]);

  return (
    <pre
      ref={ref}
      className={`code-block${wrap ? " code-block-wrap" : ""}${className ? ` ${className}` : ""}`}
      style={{ maxHeight }}
    >
      {children}
    </pre>
  );
}
