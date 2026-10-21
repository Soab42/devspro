import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useModule } from "../../app/ModuleRegistry";
import { ThemeControl } from "../../app/ThemeControl";
import { ZoomControl } from "../../app/ZoomControl";
import {
  Button,
  Callout,
  Field,
  Page,
  Section,
  Stack,
  Switch,
  Mono,
  TextInput,
} from "../../ui";
import { Monitor, Pencil, Server, ShieldCheck } from "../../ui/icons";
import {
  DEFAULT_EXTERNAL_EDITOR,
  EXTERNAL_EDITOR_KEY,
  autostartIsEnabled,
  autostartSetEnabled,
  getSetting,
  setSetting,
} from "./api";
import BackupRestoreSection from "./components/BackupRestoreSection";
import ModulesSection from "./components/ModulesSection";

export default function SettingsPage() {
  const module = useModule("settings");
  const [pingOk, setPingOk] = useState<boolean | null>(null);
  const [pingMessage, setPingMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [closeToTray, setCloseToTray] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  const [editor, setEditor] = useState(DEFAULT_EXTERNAL_EDITOR);

  useEffect(() => {
    autostartIsEnabled()
      .then(setAutostart)
      .catch(() => setAutostart(false));
    getSetting("close_to_tray")
      .then((v) => setCloseToTray(v === "true"))
      .catch(() => {});
    getSetting(EXTERNAL_EDITOR_KEY)
      .then((v) => setEditor(v?.trim() || DEFAULT_EXTERNAL_EDITOR))
      .catch(() => {});
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  async function toggleAutostart(checked: boolean) {
    setAutostart(checked);
    try {
      await autostartSetEnabled(checked);
      await setSetting("autostart_enabled", checked ? "true" : "false");
    } catch (e) {
      setAutostart(!checked);
      console.error(e);
    }
  }

  function saveEditor() {
    const value = editor.trim() || DEFAULT_EXTERNAL_EDITOR;
    setEditor(value);
    setSetting(EXTERNAL_EDITOR_KEY, value).catch(console.error);
  }

  async function toggleCloseToTray(checked: boolean) {
    setCloseToTray(checked);
    try {
      await setSetting("close_to_tray", checked ? "true" : "false");
    } catch (e) {
      setCloseToTray(!checked);
      console.error(e);
    }
  }

  async function testElevation() {
    setBusy(true);
    setPingOk(null);
    setPingMessage(null);
    try {
      const result = await invoke<string>("elevate_ping");
      setPingOk(true);
      setPingMessage(result);
    } catch (e) {
      // Previously both outcomes rendered as the same grey muted paragraph,
      // prefixed "Success:" / "Failed:" — the only thing distinguishing a
      // working elevation pipeline from a broken one was one word of text.
      setPingOk(false);
      setPingMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Page title={module.label} icon={module.icon} width="narrow">
      <Stack gap={5}>
        <Section
          title="Appearance"
          icon={Monitor}
          description='Choose how DevsPro looks. "System" follows your OS setting.'
        >
          <Field label="Theme" inline>
            {/* Exactly the control in the topbar — they used to be two separate
                hand-rolled toggles over the same state. */}
            <ThemeControl iconOnly={false} size="md" />
          </Field>
          <Field
            label="Interface size"
            inline
            hint="Scales text, forms and tables. Also Ctrl+Plus, Ctrl+Minus, Ctrl+0 and Ctrl+mouse wheel."
          >
            <ZoomControl />
          </Field>
        </Section>

        <ModulesSection />

        <Section
          title="Background running"
          icon={Server}
          description="Keep nginx and containers running via the tray icon, without a window open on your desktop the whole time."
        >
          <Stack gap={5}>
            <Switch
              checked={autostart}
              onChange={toggleAutostart}
              label="Start on login"
              description="Launch DevsPro automatically when you sign in."
            />
            <Switch
              checked={closeToTray}
              onChange={toggleCloseToTray}
              label="Keep running in tray when closed"
              description="Closing the window minimises to the tray instead of quitting."
            />
          </Stack>
        </Section>

        <Section
          title="External editor"
          icon={Pencil}
          description="The command that Open in editor runs, with the file path appended. Remote files are downloaded to a cache and uploaded again on every save."
        >
          <Field
            label="Command"
            htmlFor="external-editor"
            hint="For example code, code -w, subl, zed, or gedit."
          >
            <TextInput
              id="external-editor"
              mono
              value={editor}
              onChange={(e) => setEditor(e.target.value)}
              onBlur={saveEditor}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              }}
            />
          </Field>
        </Section>

        <Section
          title="Elevation helper"
          icon={ShieldCheck}
          description="Round-trips a harmless Ping through the real OS elevation prompt (pkexec on Linux, UAC on Windows) to confirm the manifest → helper → result pipeline works before any module relies on it for real system changes."
        >
          <Stack gap={4}>
            <div>
              <Button
                variant="secondary"
                loading={busy}
                onClick={testElevation}
              >
                Test elevation
              </Button>
            </div>
            {busy && (
              <p className="u-muted">Waiting for the elevation prompt…</p>
            )}
            {pingOk !== null && (
              <Callout tone={pingOk ? "success" : "danger"}>
                {pingMessage}
              </Callout>
            )}
          </Stack>
        </Section>

        <BackupRestoreSection />

        <Section title="About">
          <Stack gap={4}>
            <Field label="Version" inline>
              <Mono>{version ?? "unknown"}</Mono>
            </Field>
          </Stack>
        </Section>
      </Stack>
    </Page>
  );
}
