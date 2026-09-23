import {
  InputHTMLAttributes,
  ReactNode,
  TextareaHTMLAttributes,
  forwardRef,
  useId,
} from "react";
import { Search, TriangleAlert, X } from "./icons";
import { Icon } from "./Icon";
import { IconButton } from "./Button";
import "./Field.css";

/* ---------------------------------------------------------------- Field -- */

export interface FieldProps {
  label?: ReactNode;
  htmlFor?: string;
  /** Explanatory text under the label, above the control. */
  description?: ReactNode;
  /** Text under the control. */
  hint?: ReactNode;
  error?: ReactNode;
  required?: boolean;
  /** Label sits to the left instead of above — for settings rows. */
  inline?: boolean;
  children: ReactNode;
  className?: string;
}

/**
 * Labels are always rendered, never replaced by a placeholder. Placeholder-only
 * fields fail for screen readers and lose their label the moment you type,
 * which is exactly when you most want to know what you're filling in.
 */
export function Field({
  label,
  htmlFor,
  description,
  hint,
  error,
  required = false,
  inline = false,
  children,
  className,
}: FieldProps) {
  const classes = ["field", inline && "field-inline", className].filter(Boolean).join(" ");

  return (
    <div className={classes}>
      {label && (
        <div className="field-label-group">
          <label className="field-label" htmlFor={htmlFor}>
            {label}
            {required && (
              <span className="field-required" aria-hidden="true">
                *
              </span>
            )}
          </label>
          {description && <p className="field-description">{description}</p>}
        </div>
      )}
      <div className="field-control">
        {children}
        {hint && !error && <p className="field-hint">{hint}</p>}
        {error && (
          <p className="field-error" role="alert">
            <Icon icon={TriangleAlert} size="xs" />
            {error}
          </p>
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------ TextInput -- */

export interface TextInputProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean;
  mono?: boolean;
  inputSize?: "sm" | "md";
}

export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(function TextInput(
  { invalid, mono, inputSize = "md", className, ...rest },
  ref,
) {
  const classes = [
    "field-input",
    `field-input-${inputSize}`,
    mono && "u-mono",
    invalid && "field-input-invalid",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return <input {...rest} ref={ref} className={classes} aria-invalid={invalid || undefined} />;
});

export interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  invalid?: boolean;
  mono?: boolean;
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
  { invalid, mono, className, ...rest },
  ref,
) {
  const classes = [
    "field-input",
    "field-textarea",
    mono && "u-mono",
    invalid && "field-input-invalid",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <textarea {...rest} ref={ref} className={classes} aria-invalid={invalid || undefined} />
  );
});

/* ---------------------------------------------------------------- Switch -- */

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: ReactNode;
  disabled?: boolean;
  id?: string;
}

export function Switch({ checked, onChange, label, description, disabled, id }: SwitchProps) {
  const generated = useId();
  const inputId = id ?? generated;

  return (
    <div className="switch-row">
      <div className="switch-text">
        <label className="switch-label" htmlFor={inputId}>
          {label}
        </label>
        {description && <p className="switch-description">{description}</p>}
      </div>
      <button
        type="button"
        id={inputId}
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        className={`switch${checked ? " switch-on" : ""}`}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-thumb" />
      </button>
    </div>
  );
}

/* -------------------------------------------------------------- Checkbox -- */

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Accessible name. Rendered visibly unless `hideLabel`. */
  label: string;
  hideLabel?: boolean;
  /** Rendered inside the label, before the text — an icon saying what kind of
      thing this is. Part of the click target; not part of the accessible name. */
  adornment?: ReactNode;
  disabled?: boolean;
  /** Some-but-not-all selected — for a select-all in a table header. */
  indeterminate?: boolean;
  id?: string;
}

export function Checkbox({
  checked,
  onChange,
  label,
  hideLabel = false,
  adornment,
  disabled,
  indeterminate = false,
  id,
}: CheckboxProps) {
  const generated = useId();
  const inputId = id ?? generated;

  return (
    <span className="checkbox-row">
      <input
        type="checkbox"
        id={inputId}
        className="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={hideLabel ? label : undefined}
        // `indeterminate` is a DOM property with no HTML attribute, so it can
        // only be set through a ref callback.
        ref={(el) => {
          if (el) el.indeterminate = indeterminate && !checked;
        }}
        onChange={(e) => onChange(e.target.checked)}
      />
      {!hideLabel && (
        <label className="checkbox-label" htmlFor={inputId}>
          {adornment}
          {label}
        </label>
      )}
    </span>
  );
}

/* ----------------------------------------------------------- SearchInput -- */

export interface SearchInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  width?: number | string;
  autoFocus?: boolean;
  /**
   * Fill the space along the parent's main axis. True by default, which is
   * right in the toolbars this mostly lives in.
   *
   * Turn it off in a *column*, where "fill the main axis" means fill the
   * height: the box stretches, `align-items: center` floats the input in the
   * middle of it, and a filter box ends up adrift in a tall empty panel.
   */
  grow?: boolean;
  "aria-label"?: string;
}

export function SearchInput({
  value,
  onChange,
  placeholder = "Search",
  width,
  autoFocus,
  grow = true,
  "aria-label": ariaLabel,
}: SearchInputProps) {
  return (
    <div
      className={grow ? "search-input" : "search-input search-input-fixed"}
      style={{ width }}
    >
      <Icon icon={Search} size="sm" className="search-input-icon" />
      <input
        type="search"
        className="field-input search-input-control"
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        aria-label={ariaLabel ?? placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
      {value && (
        <IconButton
          icon={X}
          label="Clear search"
          size="sm"
          variant="ghost"
          className="search-input-clear"
          onClick={() => onChange("")}
        />
      )}
    </div>
  );
}

/* ------------------------------------------------------- Form structure -- */

export interface FormProps {
  onSubmit: () => void;
  children: ReactNode;
  className?: string;
}

/**
 * A real <form>, so Enter submits. Several dialogs previously used a <div> with
 * a click handler on the primary button, which silently broke keyboard submit.
 */
export function Form({ onSubmit, children, className }: FormProps) {
  return (
    <form
      className={className ? `form ${className}` : "form"}
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
    >
      {children}
    </form>
  );
}

export interface FormRowProps {
  children: ReactNode;
  align?: "center" | "end";
  className?: string;
}

/** Horizontal cluster — an input plus its adjacent button. Zero margin. */
export function FormRow({ children, align = "center", className }: FormRowProps) {
  return (
    <div
      className={`form-row form-row-${align}${className ? ` ${className}` : ""}`}
    >
      {children}
    </div>
  );
}

export interface FormGridProps {
  columns?: 1 | 2 | 3;
  children: ReactNode;
  className?: string;
}

export function FormGrid({ columns = 2, children, className }: FormGridProps) {
  return (
    <div
      className={`form-grid form-grid-${columns}${className ? ` ${className}` : ""}`}
    >
      {children}
    </div>
  );
}
