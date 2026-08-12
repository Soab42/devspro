import { useState } from "react";
import { useAuth } from "./AuthContext";
import {
  Button,
  Callout,
  Field,
  Form,
  Icon,
  Stack,
  TextInput,
} from "../ui";
import { UserPlus } from "../ui/icons";
import "./Auth.css";

export default function CreateProfile() {
  const { createProfile, error } = useAuth();
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [confirmTouched, setConfirmTouched] = useState(false);
  const [busy, setBusy] = useState(false);

  // Checked as you go rather than only on submit, so the mismatch surfaces
  // while you still remember what you typed.
  const mismatch = confirmTouched && confirm.length > 0 && password !== confirm;

  async function handleSubmit() {
    if (password !== confirm) {
      setConfirmTouched(true);
      return;
    }
    setBusy(true);
    try {
      await createProfile(displayName, password);
    } catch {
      // surfaced via context `error`
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="auth-screen">
      <div className="auth-card">
        <div className="auth-brand">
          <span className="auth-mark" aria-hidden="true">
            DP
          </span>
          <span className="auth-wordmark">DevsPro</span>
        </div>

        <Form onSubmit={handleSubmit}>
          <Stack gap={5}>
            <div>
              <h1 className="auth-title">
                <Icon icon={UserPlus} size="md" />
                Welcome
              </h1>
              <p className="auth-subtitle">Create a local profile to get started.</p>
            </div>

            {/* Real labels, not placeholders — a placeholder-only field loses
                its label the moment you type into it. */}
            <Field label="Your name" htmlFor="profile-name" required>
              <TextInput
                id="profile-name"
                autoFocus
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </Field>

            <Field label="Password" htmlFor="profile-password" required>
              <div className="dt-cell-inline">
                <TextInput
                  id="profile-password"
                  type="password"
                  autoComplete="new-password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="u-flex-1"
                />
              </div>
            </Field>

            <Field
              label="Confirm password"
              htmlFor="profile-confirm"
              required
              error={mismatch ? "Passwords do not match." : undefined}
            >
              <div className="dt-cell-inline">
                <TextInput
                  id="profile-confirm"
                  type="password"
                  autoComplete="new-password"
                  value={confirm}
                  invalid={mismatch}
                  onChange={(e) => setConfirm(e.target.value)}
                  onBlur={() => setConfirmTouched(true)}
                  className="u-flex-1"
                />
              </div>
            </Field>

            {error && <Callout tone="danger">{error}</Callout>}

            <Button
              type="submit"
              variant="primary"
              size="lg"
              fullWidth
              loading={busy}
              disabled={mismatch}
            >
              Create profile
            </Button>
          </Stack>
        </Form>
      </div>
    </div>
  );
}
