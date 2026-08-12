import { useState } from "react";
import { useAuth } from "./AuthContext";
import {
  Button,
  Callout,
  Field,
  Form,
  Icon,
  IconButton,
  Select,
  Stack,
  TextInput,
} from "../ui";
import { Eye, EyeOff, Lock } from "../ui/icons";
import "./Auth.css";

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

export default function LockScreen() {
  const { profiles, unlock, error } = useAuth();
  const [profileId, setProfileId] = useState(profiles[0]?.id ?? "");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);

  const activeId = profileId || profiles[0]?.id;
  const activeProfile = profiles.find((p) => p.id === activeId) ?? profiles[0];

  async function handleSubmit() {
    if (!activeId) return;
    setBusy(true);
    try {
      await unlock(activeId, password);
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
                <Icon icon={Lock} size="md" />
                Locked
              </h1>
              <p className="auth-subtitle">Enter your password to continue.</p>
            </div>

            {profiles.length > 1 ? (
              <Field label="Profile" htmlFor="lock-profile">
                {/* Was the last native <select> in the app — under WebKitGTK it
                    renders with GTK's own colours and ignores our theme, which
                    is the exact bug the custom Select exists to avoid. */}
                <Select
                  id="lock-profile"
                  value={activeId ?? ""}
                  onChange={setProfileId}
                  options={profiles.map((p) => ({
                    value: p.id,
                    label: p.display_name,
                  }))}
                  aria-label="Profile"
                />
              </Field>
            ) : (
              activeProfile && (
                <div className="auth-identity">
                  <span className="auth-avatar" aria-hidden="true">
                    {initials(activeProfile.display_name)}
                  </span>
                  <span className="auth-identity-name">
                    {activeProfile.display_name}
                  </span>
                </div>
              )
            )}

            <Field label="Password" htmlFor="lock-password">
              <div className="dt-cell-inline">
                <TextInput
                  id="lock-password"
                  type={reveal ? "text" : "password"}
                  autoFocus
                  autoComplete="current-password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="u-flex-1"
                />
                <IconButton
                  icon={reveal ? EyeOff : Eye}
                  label={reveal ? "Hide password" : "Show password"}
                  onClick={() => setReveal((v) => !v)}
                />
              </div>
            </Field>

            {error && <Callout tone="danger">{error}</Callout>}

            <Button type="submit" variant="primary" size="lg" fullWidth loading={busy}>
              Unlock
            </Button>
          </Stack>
        </Form>
      </div>
    </div>
  );
}
