import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Field, Form, FormGrid, FormRow, Select, Stack, TextInput } from "../../../ui";
import { AuthMethod, KeyPermissions, SshHost, SshKey, configIdentities, keyPermissions, restrictKey, saveHost } from "../api";
import { errorMessage, keysIgnoredByAgentMode, validateHostForm } from "../utils";

const AUTH_OPTIONS: { value: AuthMethod; label: string }[] = [
  { value: "agent", label: "SSH agent / default keys" },
  { value: "key", label: "Private key file" },
  { value: "password", label: "Password (asked on connect)" },
];

export default function HostForm({
  host,
  keys,
  onSaved,
  onCancel,
}: {
  host?: SshHost;
  keys: SshKey[];
  onSaved: (saved: SshHost) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(host?.name ?? "");
  const [address, setAddress] = useState(host?.host ?? "");
  const [port, setPort] = useState(String(host?.port ?? 22));
  const [username, setUsername] = useState(host?.username ?? "");
  const [auth, setAuth] = useState<AuthMethod>(host?.auth ?? "agent");
  const [keyPath, setKeyPath] = useState(host?.key_path ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const localKeys = keys.filter((k) => k.has_private);

  // The terminal's ssh refuses a key others can read, though SFTP doesn't
  // mind; say so here rather than leave it to a failed terminal. The key's
  // permissions while it is too open, else null.
  const keyToCheck = auth === "key" ? keyPath.trim() : "";
  const [openMode, setOpenMode] = useState<KeyPermissions | null>(null);
  useEffect(() => {
    setOpenMode(null);
    if (!keyToCheck) return;
    let live = true;
    keyPermissions(keyToCheck).then(
      (p) => live && setOpenMode(p.too_open ? p : null),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [keyToCheck]);

  // "SSH agent / default keys" never offers a key with any other name unless
  // ~/.ssh/config names it for this host — say which keys that leaves out,
  // rather than let the first connect fail with "Permission denied".
  const [configKeys, setConfigKeys] = useState<string[] | null>(null);
  const lookupHost = auth === "agent" ? address.trim() : "";
  const lookupUser = username.trim();
  const lookupPort = Number(port) || 22;
  useEffect(() => {
    // The last answer stays up while a new one is fetched, so typing the
    // address doesn't make the warning blink.
    if (auth !== "agent") return;
    let live = true;
    const timer = window.setTimeout(() => {
      configIdentities(lookupHost, lookupPort, lookupUser).then(
        (paths) => live && setConfigKeys(paths),
        () => live && setConfigKeys([]),
      );
    }, 250);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [auth, lookupHost, lookupPort, lookupUser]);
  const ignoredKeys = auth === "agent" && configKeys !== null ? keysIgnoredByAgentMode(keys, configKeys) : [];

  function switchToKeyFile() {
    setAuth("key");
    if (ignoredKeys.length === 1) setKeyPath(ignoredKeys[0].path);
  }

  async function handleRestrict() {
    try {
      await restrictKey(keyToCheck);
      setOpenMode(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function handleBrowse() {
    const selected = await open({ multiple: false, title: "Choose a private key" });
    if (!selected || Array.isArray(selected)) return;
    setKeyPath(selected);
  }

  async function handleSubmit() {
    const problem = validateHostForm({ name, host: address, port, username, auth, keyPath });
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await saveHost(host?.id ?? null, {
        name: name.trim(),
        host: address.trim(),
        port: Number(port),
        username: username.trim(),
        auth,
        keyPath: auth === "key" ? keyPath.trim() : null,
      });
      onSaved(saved);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field label="Name" htmlFor="ssh-host-name" required>
          <TextInput
            id="ssh-host-name"
            placeholder="Production"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <FormGrid columns={3}>
          <Field label="Host" htmlFor="ssh-host-address" required>
            <TextInput
              id="ssh-host-address"
              mono
              placeholder="example.com"
              value={address}
              onChange={(e) => setAddress(e.target.value)}
            />
          </Field>
          <Field label="Port" htmlFor="ssh-host-port" required>
            <TextInput
              id="ssh-host-port"
              mono
              inputMode="numeric"
              value={port}
              onChange={(e) => setPort(e.target.value)}
            />
          </Field>
          <Field label="Username" htmlFor="ssh-host-user" required>
            <TextInput
              id="ssh-host-user"
              mono
              placeholder="deploy"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </Field>
        </FormGrid>

        <Field
          label="Authentication"
          htmlFor="ssh-host-auth"
          description="Passwords are never saved; you are asked for one each time you connect."
        >
          <Select
            id="ssh-host-auth"
            value={auth}
            onChange={(v) => setAuth(v as AuthMethod)}
            options={AUTH_OPTIONS}
          />
        </Field>

        {ignoredKeys.length > 0 && (
          <Callout
            tone="warning"
            actions={
              <Button size="sm" variant="secondary" onClick={switchToKeyFile}>
                {ignoredKeys.length === 1 ? `Use ${ignoredKeys[0].name}` : "Pick a key file"}
              </Button>
            }
          >
            This mode only offers keys loaded in ssh-agent, keys <code>~/.ssh/config</code> names for this host,
            and <code>id_ed25519</code> / <code>id_ecdsa</code> / <code>id_rsa</code>. So{" "}
            {ignoredKeys.map((k, i) => (
              <span key={k.path}>
                {i > 0 && ", "}
                <code>{k.name}</code>
              </span>
            ))}{" "}
            in <code>~/.ssh</code> won't be used. If this host needs {ignoredKeys.length === 1 ? "it" : "one of them"},
            switch to a private key file.
          </Callout>
        )}

        {auth === "key" && (
          <Field label="Private key" htmlFor="ssh-host-key" required>
            <Stack gap={3}>
              {localKeys.length > 0 && (
                <Select
                  aria-label="Choose a key from ~/.ssh"
                  value={localKeys.some((k) => k.path === keyPath) ? keyPath : ""}
                  onChange={setKeyPath}
                  placeholder="Pick a key from ~/.ssh…"
                  options={localKeys.map((k) => ({
                    value: k.path,
                    label: k.comment ? `${k.name} — ${k.comment}` : k.name,
                  }))}
                />
              )}
              <FormRow>
                <TextInput
                  id="ssh-host-key"
                  mono
                  placeholder="/home/you/.ssh/id_ed25519"
                  value={keyPath}
                  onChange={(e) => setKeyPath(e.target.value)}
                  className="u-flex-1"
                />
                <Button variant="secondary" onClick={handleBrowse}>
                  Choose file…
                </Button>
              </FormRow>
              {openMode !== null && (
                <Callout
                  tone="warning"
                  actions={
                    <Button size="sm" variant="secondary" onClick={() => void handleRestrict()}>
                      Fix
                    </Button>
                  }
                >
                  {openMode.others?.length ? (
                    <>
                      This key is open to {openMode.others.join(", ")}, so ssh will refuse it in the terminal. Fix
                      restricts it to you.
                    </>
                  ) : (
                    <>
                      Other users can read this key (mode {openMode.mode.toString(8)}), so ssh will refuse it in the
                      terminal. Fix restricts it to you (chmod 600).
                    </>
                  )}
                </Callout>
              )}
            </Stack>
          </Field>
        )}

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {host ? "Save changes" : "Add host"}
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
