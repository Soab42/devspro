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
import { NewPasswordFields, useNewPassword } from "./NewPasswordFields";
import "./Auth.css";

export default function CreateProfile() {
  const { createProfile, error } = useAuth();
  const [displayName, setDisplayName] = useState("");
  const pw = useNewPassword();
  const [busy, setBusy] = useState(false);

  async function create(name: string, secret: string) {
    setBusy(true);
    try {
      await createProfile(name, secret);
    } catch {
      // surfaced via context `error`
    } finally {
      setBusy(false);
    }
  }

  async function handleSubmit() {
    if (!pw.confirmed()) return;
    await create(displayName, pw.password);
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
              <p className="auth-subtitle">
                Create a local profile, or skip it. A password is optional: without one, DevsPro opens
                straight away, protected by your computer's own login.
              </p>
            </div>

            {/* Real labels, not placeholders — a placeholder-only field loses
                its label the moment you type into it. */}
            <Field label="Your name" htmlFor="profile-name" description="Optional. Your computer's account name is used when left empty.">
              <TextInput
                id="profile-name"
                autoFocus
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
              />
            </Field>

            <NewPasswordFields state={pw} idPrefix="profile" description="Optional. Leave empty to open without one." />

            {error && <Callout tone="danger">{error}</Callout>}

            <Button
              type="submit"
              variant="primary"
              size="lg"
              fullWidth
              loading={busy}
              disabled={pw.mismatch}
            >
              {pw.password.length > 0 ? "Create profile" : "Continue without a password"}
            </Button>
            <Button variant="link" fullWidth disabled={busy} onClick={() => void create("", "")}>
              Skip — use DevsPro without a profile
            </Button>
          </Stack>
        </Form>
      </div>
    </div>
  );
}
