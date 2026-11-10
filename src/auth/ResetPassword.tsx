import { useState } from "react";
import { useAuth, type Profile } from "./AuthContext";
import { Button, Callout, Form, Icon, Stack } from "../ui";
import { KeyRound } from "../ui/icons";
import { NewPasswordFields, useNewPassword } from "./NewPasswordFields";
import "./Auth.css";

/**
 * The lock screen's "Forgot password?". The OS administrator prompt stands in
 * for the forgotten password: the password guards nothing the computer's own
 * account can't read, and encrypts nothing, so a reset loses no data.
 */
export default function ResetPassword({ profile, onBack }: { profile: Profile; onBack: () => void }) {
  const { resetPassword } = useAuth();
  const pw = useNewPassword();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit() {
    if (!pw.confirmed()) return;
    setBusy(true);
    setError(null);
    try {
      // On success the profile opens and this screen goes away.
      await resetPassword(profile.id, pw.password);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <div>
          <h1 className="auth-title">
            <Icon icon={KeyRound} size="md" />
            Reset password
          </h1>
          <p className="auth-subtitle">
            Your computer asks for an administrator's approval, then {profile.display_name} gets the password you
            choose here, or none. Nothing else changes: your sites, hosts and saved logins stay as they are.
          </p>
        </div>

        <NewPasswordFields
          state={pw}
          idPrefix="reset"
          label="New password"
          description="Optional. Leave empty to open without one."
          autoFocus
        />

        {error && <Callout tone="danger">{error}</Callout>}

        <Button type="submit" variant="primary" size="lg" fullWidth loading={busy} disabled={pw.mismatch}>
          {pw.password.length > 0 ? "Reset password" : "Remove password"}
        </Button>
        <Button variant="link" fullWidth disabled={busy} onClick={onBack}>
          Back to unlock
        </Button>
      </Stack>
    </Form>
  );
}
