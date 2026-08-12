import { FormEvent, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import StatusPill from "../../../components/StatusPill";
import Select from "../../../components/Select";
import {
  addLaravelSite,
  addPortSite,
  detectPhpFpm,
  hostnameConflict,
  suggestPort,
  validateLaravelProject,
} from "../api";

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

export default function AddSiteForm({ onAdded }: { onAdded: () => void }) {
  const [name, setName] = useState("");
  const [hostname, setHostname] = useState("");
  const [kind, setKind] = useState<BackendKind>("port");
  const [port, setPort] = useState("");
  const [projectPath, setProjectPath] = useState("");
  const [projectStatus, setProjectStatus] = useState<"idle" | "checking" | "ok" | "invalid">("idle");
  const [phpFpmMissing, setPhpFpmMissing] = useState(false);
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
      const detection = await detectPhpFpm();
      setPhpFpmMissing(!detection.found);
    }
  }

  async function handleBrowseFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Select your Laravel project folder" });
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
    } catch (e) {
      setProjectStatus("invalid");
      setError(String(e));
    }
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (!name.trim() || !hostname.trim()) {
      setError("name and hostname are required");
      return;
    }
    setBusy(true);
    try {
      if (kind === "port") {
        const portNum = Number(port);
        if (!portNum) {
          setError("port is required");
          setBusy(false);
          return;
        }
        await addPortSite(name.trim(), hostname.trim(), portNum);
      } else {
        if (!projectPath || projectStatus !== "ok") {
          setError("choose a valid Laravel project folder first");
          setBusy(false);
          return;
        }
        await addLaravelSite(name.trim(), hostname.trim(), projectPath);
      }
      onAdded();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={handleSubmit}>
      <div className="form-field">
        <label htmlFor="site-name">Name</label>
        <input id="site-name" placeholder="My App" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>

      <div className="form-field">
        <label htmlFor="site-hostname">Hostname</label>
        <input
          id="site-hostname"
          placeholder="myapp.test"
          value={hostname}
          onChange={(e) => setHostname(e.target.value)}
          onBlur={handleHostnameBlur}
        />
      </div>

      <div className="form-field">
        <label htmlFor="site-kind">Backend</label>
        <Select
          id="site-kind"
          value={kind}
          onChange={(v) => handleSelectKind(v as BackendKind)}
          options={[
            { value: "port", label: "Proxy to a port" },
            { value: "laravel", label: "Laravel project (auto)" },
          ]}
        />
      </div>

      {kind === "port" ? (
        <div className="form-row">
          <input placeholder="Port" value={port} onChange={(e) => setPort(e.target.value)} style={{ maxWidth: "90px" }} />
          <button type="button" className="icon-button secondary" onClick={handleSuggestPort}>
            Suggest port
          </button>
        </div>
      ) : (
        <div className="form-row">
          <button type="button" className="icon-button secondary" onClick={handleBrowseFolder}>
            Choose folder…
          </button>
          {projectStatus === "ok" && <StatusPill ok label="Laravel project detected" />}
          {projectStatus === "invalid" && <StatusPill ok={false} label="not a Laravel project" />}
        </div>
      )}
      {kind === "laravel" && projectPath && <p className="muted">{projectPath}</p>}

      {kind === "laravel" && phpFpmMissing && (
        <p className="field-error">
          PHP-FPM was not found — install it first (e.g. <code>sudo apt install php8.3-fpm</code>).
        </p>
      )}
      {conflict && (
        <p className="field-error">
          Warning: {hostname} already resolves to something else in /etc/hosts outside of DevSuite's block.
        </p>
      )}
      {error && <p className="field-error">{error}</p>}

      <div className="form-row">
        <button type="submit" disabled={busy}>
          {busy ? "Waiting for admin password…" : "Add site"}
        </button>
      </div>
    </form>
  );
}
