import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Mono,
  Select,
  SegmentedControl,
  Spinner,
  Stack,
  StatusPill,
  TextInput,
} from "../../../ui";
import { WandSparkles } from "../../../ui/icons";
import {
  addLaravelSite,
  addPortSite,
  detectPhpFpm,
  hostnameConflict,
  listPhpVersions,
  PhpFpmVersion,
  suggestPort,
  validateLaravelProject,
} from "../api";

const SYSTEM_DEFAULT = "";

type BackendKind = "port" | "laravel";

function folderBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function slugForHostname(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export default function AddSiteForm({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [hostname, setHostname] = useState("");
  const [kind, setKind] = useState<BackendKind>("port");
  const [port, setPort] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [projectStatus, setProjectStatus] = useState<
    "idle" | "checking" | "ok" | "invalid"
  >("idle");
  const [phpFpmMissing, setPhpFpmMissing] = useState(false);
  // Supplied by the backend rather than written here: what to install and how depends on
  // the OS, which the frontend has no way to know.
  const [phpHint, setPhpHint] = useState("");
  const [phpVersions, setPhpVersions] = useState<PhpFpmVersion[]>([]);
  const [phpVersion, setPhpVersion] = useState(SYSTEM_DEFAULT);
  const [conflict, setConflict] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleHostnameBlur() {
    if (!hostname) return;
    setConflict(await hostnameConflict(hostname));
  }

  async function handleSuggestPort() {
    const suggestion = await suggestPort(3000);
    if (suggestion) setPort(String(suggestion));
  }

  async function handleSelectKind(next: BackendKind) {
    setKind(next);
    setError(null);
    if (next === "laravel") {
      const [detection, versions] = await Promise.all([detectPhpFpm(), listPhpVersions()]);
      setPhpFpmMissing(!detection.found);
      setPhpHint(detection.install_hint);
      setPhpVersions(versions);
    }
  }

  async function handleBrowseFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select your Laravel project folder",
    });
    if (!selected || Array.isArray(selected)) return;

    setProjectPath(selected);
    setProjectStatus("checking");

    const folderName = folderBasename(selected);
    if (!name.trim()) setName(folderName);
    if (!hostname.trim()) {
      const slug = slugForHostname(folderName);
      if (slug) setHostname(`${slug}.test`);
    }

    try {
      await validateLaravelProject(selected);
      setProjectStatus("ok");
      setError(null);
    } catch (e) {
      setProjectStatus("invalid");
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function handleSubmit() {
    setError(null);
    if (!name.trim() || !hostname.trim()) {
      setError("Name and hostname are both required.");
      return;
    }

    setBusy(true);
    try {
      if (kind === "port") {
        const portNum = Number(port);
        if (!portNum) {
          setError("A port is required.");
          setBusy(false);
          return;
        }
        await addPortSite(name.trim(), hostname.trim(), portNum);
      } else {
        if (!projectPath || projectStatus !== "ok") {
          setError("Choose a valid Laravel project folder first.");
          setBusy(false);
          return;
        }
        await addLaravelSite(
          name.trim(),
          hostname.trim(),
          projectPath,
          phpVersion || null,
        );
      }
      onAdded();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field label="Name" htmlFor="site-name" required>
          <TextInput
            id="site-name"
            placeholder="My App"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </Field>

        <Field
          label="Hostname"
          htmlFor="site-hostname"
          required
          error={
            conflict
              ? `${hostname} already resolves to something else in /etc/hosts, outside DevsPro's block.`
              : undefined
          }
        >
          <TextInput
            id="site-hostname"
            mono
            placeholder="myapp.test"
            value={hostname}
            invalid={conflict}
            onChange={(e) => setHostname(e.target.value)}
            onBlur={handleHostnameBlur}
          />
        </Field>

        {/* Two mutually exclusive modes — a segmented control says that far
            better than a dropdown of two items. */}
        <Field label="Backend">
          <SegmentedControl
            ariaLabel="Backend type"
            size="md"
            value={kind}
            onChange={handleSelectKind}
            options={[
              { value: "port", label: "Proxy to a port" },
              { value: "laravel", label: "Laravel project" },
            ]}
          />
        </Field>

        {kind === "port" ? (
          <Field label="Port" htmlFor="site-port" required>
            <FormRow>
              <TextInput
                id="site-port"
                mono
                inputMode="numeric"
                className="field-input-narrow"
                placeholder="3000"
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
              <Button
                variant="secondary"
                icon={WandSparkles}
                onClick={handleSuggestPort}
              >
                Suggest
              </Button>
            </FormRow>
          </Field>
        ) : (
          <Field
            label="Project folder"
            required
            hint={projectPath ? undefined : "Pick the folder containing artisan."}
          >
            <Stack gap={3}>
              <FormRow>
                <Button variant="secondary" onClick={handleBrowseFolder}>
                  Choose folder…
                </Button>
                {/* Validation state, not a status: spinner while checking, then
                    a pass/fail pill rather than a green "ok" for both. */}
                {projectStatus === "checking" && <Spinner label="Checking project" />}
                {projectStatus === "ok" && (
                  <StatusPill variant="ok" label="Laravel project" />
                )}
                {projectStatus === "invalid" && (
                  <StatusPill variant="bad" label="not a Laravel project" />
                )}
              </FormRow>
              {projectPath && <Mono truncate>{projectPath}</Mono>}
            </Stack>
          </Field>
        )}

        {kind === "laravel" && phpVersions.length > 0 && (
          <Field label="PHP version" htmlFor="site-php-version">
            <Select
              id="site-php-version"
              value={phpVersion}
              onChange={setPhpVersion}
              options={[
                { value: SYSTEM_DEFAULT, label: "System default" },
                ...phpVersions.map((v) => ({
                  value: v.version,
                  label: `PHP ${v.version}`,
                })),
              ]}
            />
          </Field>
        )}

        {kind === "laravel" && phpFpmMissing && <Callout tone="warning">{phpHint}</Callout>}

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          {busy && <span className="u-muted">Waiting for your admin password…</span>}
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            Add site
          </Button>
        </FormRow>
      </Stack>
    </Form>
  );
}
