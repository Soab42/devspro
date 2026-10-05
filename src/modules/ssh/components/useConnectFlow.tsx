import { ReactNode, useCallback, useRef, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { Mono, Stack } from "../../../ui";
import { Credentials, SshHost, keyPermissions, listHosts, restrictKey, sftpTrustHost } from "../api";
import { errorMessage, hostLabel, isPasswordRequired, unknownHostFingerprint } from "../utils";
import CredentialsPrompt, { PromptKind } from "./CredentialsPrompt";

interface PendingPrompt {
  host: SshHost;
  kind: PromptKind;
  error: string | null;
  resolve: (value: string | null) => void;
}

const MAX_ATTEMPTS = 6;

/**
 * Runs an ssh2-backed operation (SFTP connect, key install) with the
 * interactive steps a first connection needs: an unknown host key becomes a
 * trust dialog, a password host gets a password prompt, an encrypted key gets
 * a passphrase prompt, and a rejected secret is asked for again with the
 * reason shown. The terminal tab doesn't need any of this — there the real
 * `ssh` asks its own questions in the PTY — except `prepareOpenSsh`, below.
 *
 * `run` returns `null` when the user cancels at any prompt; rethrows anything else.
 */
export function useConnectFlow() {
  const confirmAction = useConfirm();
  const [prompt, setPrompt] = useState<PendingPrompt | null>(null);
  const promptRef = useRef<PendingPrompt | null>(null);

  const ask = useCallback((host: SshHost, kind: PromptKind, error: string | null) => {
    return new Promise<string | null>((resolve) => {
      const pending = { host, kind, error, resolve };
      promptRef.current = pending;
      setPrompt(pending);
    });
  }, []);

  const settle = useCallback((value: string | null) => {
    promptRef.current?.resolve(value);
    promptRef.current = null;
    setPrompt(null);
  }, []);

  const run = useCallback(
    async <T,>(host: SshHost, attempt: (creds: Credentials) => Promise<T>): Promise<T | null> => {
      const creds: Credentials = {};
      if (host.auth === "password") {
        const password = await ask(host, "password", null);
        if (password === null) return null;
        creds.password = password;
      }

      for (let tries = 0; tries < MAX_ATTEMPTS; tries += 1) {
        try {
          return await attempt(creds);
        } catch (e) {
          const message = errorMessage(e);

          const fingerprint = unknownHostFingerprint(message);
          if (fingerprint) {
            const ok = await confirmAction(
              <Stack gap={3}>
                <span>
                  The authenticity of <strong>{hostLabel(host)}</strong> can't be established. Its host key
                  fingerprint is:
                </span>
                <Mono copyable copyValue={fingerprint}>
                  {fingerprint}
                </Mono>
                <span>Compare it with what the server's administrator gave you before trusting it.</span>
              </Stack>,
              { title: "Trust this host?", confirmLabel: "Trust and connect" },
            );
            if (!ok) return null;
            await sftpTrustHost(host.id);
            continue;
          }

          if (isPasswordRequired(message) || /password rejected/i.test(message)) {
            const password = await ask(host, "password", tries > 0 || creds.password ? "Password rejected." : null);
            if (password === null) return null;
            creds.password = password;
            continue;
          }

          if (host.auth === "key" && /key auth failed/i.test(message)) {
            const passphrase = await ask(
              host,
              "passphrase",
              creds.passphrase !== undefined ? "The key could not be unlocked with that passphrase." : null,
            );
            if (passphrase === null) return null;
            creds.passphrase = passphrase;
            continue;
          }

          throw e;
        }
      }
      throw new Error("Too many failed attempts.");
    },
    [ask, confirmAction],
  );

  /**
   * Run before starting the system `ssh` for a saved host. ssh refuses a key
   * file that group or others can access, which ssh2 (SFTP) doesn't check,
   * so this offers the `chmod 600` that fixes it. Resolves false when the
   * user declines, and the caller doesn't start ssh. The host is read fresh,
   * so a key changed since the caller's list loaded is the one checked.
   */
  const prepareOpenSsh = useCallback(
    async (hostId: string): Promise<boolean> => {
      const host = (await listHosts().catch(() => [])).find((h) => h.id === hostId);
      const path = host?.auth === "key" ? host.key_path?.trim() : null;
      if (!path) return true;
      // A missing or unreadable key is ssh's to report, in the terminal.
      const perms = await keyPermissions(path).catch(() => null);
      if (!perms?.too_open) return true;
      const ok = await confirmAction(
        <span>
          <Mono>{path}</Mono> can be read by other users (mode {perms.mode.toString(8)}), so ssh will refuse it.
          Restrict it to you (chmod 600) and connect?
        </span>,
        { title: "Private key is too open", confirmLabel: "Restrict and connect" },
      );
      if (!ok) return false;
      await restrictKey(path);
      return true;
    },
    [confirmAction],
  );

  const element: ReactNode = prompt ? (
    <CredentialsPrompt
      host={prompt.host}
      kind={prompt.kind}
      error={prompt.error}
      onSubmit={(value) => settle(value)}
      onCancel={() => settle(null)}
    />
  ) : null;

  return { run, prepareOpenSsh, element };
}
