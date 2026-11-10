import { useState } from "react";
import { useAuth } from "../../../auth/AuthContext";
import { NewPasswordFields, useNewPassword } from "../../../auth/NewPasswordFields";
import { Button, Callout, Field, Form, Section, Stack, TextInput } from "../../../ui";
import { KeyRound } from "../../../ui/icons";

/** Sets, changes or removes the open profile's password. */
export default function PasswordSection() {
  const { profile, changePassword } = useAuth();
  const [current, setCurrent] = useState("");
  const pw = useNewPassword();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  if (!profile) return null;
  const hasPassword = profile.has_password;
  const removing = hasPassword && pw.password.length === 0;

  async function handleSubmit() {
    if (!pw.confirmed() || (!hasPassword && pw.password.length === 0)) return;
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      await changePassword(current, pw.password);
      setDone(removing ? "Password removed. DevsPro now opens without asking." : hasPassword ? "Password changed." : "Password set.");
      setCurrent("");
      pw.clear();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Section
      title="Password"
      icon={KeyRound}
      description={
        hasPassword
          ? "DevsPro asks for it when this profile is locked. Forgot it? The lock screen can reset it with your computer's administrator approval."
          : "This profile has none, so DevsPro opens without asking. Set one to lock it."
      }
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={4}>
          {hasPassword && (
            <Field label="Current password" htmlFor="settings-current-password">
              <div className="dt-cell-inline">
                <TextInput
                  id="settings-current-password"
                  type="password"
                  autoComplete="current-password"
                  value={current}
                  onChange={(e) => setCurrent(e.target.value)}
                  className="u-flex-1"
                />
              </div>
            </Field>
          )}

          <NewPasswordFields
            state={pw}
            idPrefix="settings"
            label="New password"
            description={hasPassword ? "Leave empty to remove the password." : undefined}
          />

          {error && <Callout tone="danger">{error}</Callout>}
          {done && <Callout tone="success">{done}</Callout>}

          <div>
            <Button
              type="submit"
              variant={removing ? "secondary" : "primary"}
              loading={busy}
              disabled={pw.mismatch || (hasPassword ? current.length === 0 : pw.password.length === 0)}
            >
              {removing ? "Remove password" : hasPassword ? "Change password" : "Set password"}
            </Button>
          </div>
        </Stack>
      </Form>
    </Section>
  );
}
