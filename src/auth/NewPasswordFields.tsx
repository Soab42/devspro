import { useState } from "react";
import { Field, TextInput } from "../ui";

/**
 * A password being chosen, and its confirmation. Empty means none, and the
 * confirm field only appears once something is typed. Shared by the first
 * launch, the lock screen's reset and Settings.
 */
export function useNewPassword() {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [confirmTouched, setConfirmTouched] = useState(false);

  // Checked as you go rather than only on submit, so the mismatch surfaces
  // while you still remember what you typed.
  const mismatch = confirmTouched && password.length > 0 && confirm.length > 0 && password !== confirm;

  return {
    password,
    confirm,
    mismatch,
    setPassword,
    setConfirm,
    touchConfirm: () => setConfirmTouched(true),
    /**
     * Whether it can be submitted; shows the mismatch when not. A confirm
     * left over from a password since cleared doesn't count.
     */
    confirmed(): boolean {
      if (password.length > 0 && password !== confirm) {
        setConfirmTouched(true);
        return false;
      }
      return true;
    },
    clear() {
      setPassword("");
      setConfirm("");
      setConfirmTouched(false);
    },
  };
}

export type NewPassword = ReturnType<typeof useNewPassword>;

interface NewPasswordFieldsProps {
  state: NewPassword;
  /** Prefixes the inputs' ids: `<prefix>-password` and `<prefix>-confirm`. */
  idPrefix: string;
  /** "Password" by default; the confirm field is labelled "Confirm <label>". */
  label?: string;
  description?: string;
  autoFocus?: boolean;
}

export function NewPasswordFields({ state, idPrefix, label = "Password", description, autoFocus }: NewPasswordFieldsProps) {
  return (
    <>
      <Field label={label} htmlFor={`${idPrefix}-password`} description={description}>
        <div className="dt-cell-inline">
          <TextInput
            id={`${idPrefix}-password`}
            type="password"
            autoComplete="new-password"
            autoFocus={autoFocus}
            value={state.password}
            onChange={(e) => state.setPassword(e.target.value)}
            className="u-flex-1"
          />
        </div>
      </Field>

      {state.password.length > 0 && (
        <Field
          label={`Confirm ${label.toLowerCase()}`}
          htmlFor={`${idPrefix}-confirm`}
          required
          error={state.mismatch ? "Passwords do not match." : undefined}
        >
          <div className="dt-cell-inline">
            <TextInput
              id={`${idPrefix}-confirm`}
              type="password"
              autoComplete="new-password"
              value={state.confirm}
              invalid={state.mismatch}
              onChange={(e) => state.setConfirm(e.target.value)}
              onBlur={state.touchConfirm}
              className="u-flex-1"
            />
          </div>
        </Field>
      )}
    </>
  );
}
