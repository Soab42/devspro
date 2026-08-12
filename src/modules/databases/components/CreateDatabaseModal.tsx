import { FormEvent, useState } from "react";
import Modal from "../../../components/Modal";
import Select, { SelectOption } from "../../../components/Select";
import { createDatabase, CreateDatabaseResult, DbKind, suggestPort } from "../api";
import { generatePassword } from "../utils";

const KIND_OPTIONS: SelectOption[] = [
  { value: "mysql", label: "MySQL" },
  { value: "postgres", label: "PostgreSQL" },
  { value: "mongo", label: "MongoDB" },
  { value: "redis", label: "Redis" },
];

const VERSION_OPTIONS: Record<DbKind, SelectOption[]> = {
  mysql: [
    { value: "8.0", label: "8.0 (default)" },
    { value: "8.4", label: "8.4" },
    { value: "5.7", label: "5.7" },
  ],
  postgres: [
    { value: "16", label: "16 (default)" },
    { value: "15", label: "15" },
    { value: "14", label: "14" },
    { value: "13", label: "13" },
  ],
  mongo: [
    { value: "7", label: "7 (default)" },
    { value: "6", label: "6" },
    { value: "5", label: "5" },
  ],
  redis: [
    { value: "7-alpine", label: "7 (default)" },
    { value: "6-alpine", label: "6" },
    { value: "latest", label: "latest" },
  ],
};

const DEFAULT_PORTS: Record<DbKind, number> = {
  mysql: 3306,
  postgres: 5432,
  mongo: 27017,
  redis: 6379,
};

const UI_LABEL: Record<DbKind, string> = {
  mysql: "Adminer",
  postgres: "Adminer",
  mongo: "Mongo Express",
  redis: "Redis Commander",
};

const DEFAULT_USERNAME: Record<DbKind, string> = {
  mysql: "root",
  postgres: "postgres",
  mongo: "root",
  redis: "",
};

export default function CreateDatabaseModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<DbKind>("mysql");
  const [version, setVersion] = useState(VERSION_OPTIONS.mysql[0].value);
  const [port, setPort] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [withUi, setWithUi] = useState(true);
  const [uiPort, setUiPort] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<CreateDatabaseResult | null>(null);
  const [copied, setCopied] = useState(false);

  function handleKindChange(next: string) {
    const nextKind = next as DbKind;
    setKind(nextKind);
    setVersion(VERSION_OPTIONS[nextKind][0].value);
    setPort("");
    setUiPort("");
  }

  async function handleSuggestPort(target: "db" | "ui") {
    const suggestion = await suggestPort(DEFAULT_PORTS[kind]);
    if (!suggestion) return;
    if (target === "db") setPort(String(suggestion));
    else setUiPort(String(suggestion));
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const portNum = Number(port);
    if (!name.trim() || !portNum) {
      setError("name and port are required");
      return;
    }
    const uiPortNum = withUi ? Number(uiPort) || null : null;
    setBusy(true);
    try {
      const created = await createDatabase({
        name: name.trim(),
        kind,
        version,
        port: portNum,
        username: username.trim() || null,
        password: password.trim() || null,
        withUi,
        uiPort: uiPortNum,
      });
      setResult(created);
      onCreated();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  if (result) {
    return (
      <Modal title="Database created" onClose={onClose}>
        <p className="muted">{result.message}</p>
        {result.username && (
          <div className="form-field">
            <label>Username</label>
            <input readOnly value={result.username} />
          </div>
        )}
        <div className="form-field">
          <label>Password</label>
          <div className="form-row">
            <input readOnly value={result.password} />
            <button type="button" onClick={() => copyText(result.password)}>
              {copied ? "Copied!" : "Copy"}
            </button>
          </div>
        </div>
        <p className="muted">
          Connect on <code>localhost:{result.db_port}</code>
          {result.ui_port && (
            <>
              {" "}
              — admin UI at <code>localhost:{result.ui_port}</code>
            </>
          )}
          . This password is saved in DevSuite's compose file — use "Reveal password" later if you lose it.
        </p>
        <div className="form-row">
          <button onClick={onClose}>Done</button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title="Create a database" onClose={onClose}>
      <form onSubmit={handleSubmit}>
        <div className="form-field">
          <label>Type</label>
          <Select value={kind} onChange={handleKindChange} options={KIND_OPTIONS} />
        </div>

        <div className="form-field">
          <label>Version</label>
          <Select value={version} onChange={setVersion} options={VERSION_OPTIONS[kind]} />
        </div>

        <div className="form-field">
          <label htmlFor="db-name">Name</label>
          <input
            id="db-name"
            placeholder="e.g. myapp_db"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
          />
        </div>

        <div className="form-field">
          <label htmlFor="db-port">Port</label>
          <div className="form-row">
            <input
              id="db-port"
              placeholder={String(DEFAULT_PORTS[kind])}
              value={port}
              onChange={(e) => setPort(e.target.value)}
              style={{ maxWidth: "100px" }}
            />
            <button type="button" className="secondary" onClick={() => handleSuggestPort("db")}>
              Suggest port
            </button>
          </div>
        </div>

        {kind !== "redis" && (
          <div className="form-field">
            <label htmlFor="db-username">Username</label>
            <input
              id="db-username"
              placeholder={DEFAULT_USERNAME[kind]}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
        )}

        <div className="form-field">
          <label htmlFor="db-password">Password</label>
          <div className="form-row">
            <input
              id="db-password"
              placeholder="leave blank to auto-generate"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
            <button type="button" className="secondary" onClick={() => setPassword(generatePassword())}>
              Generate
            </button>
          </div>
        </div>

        <div className="form-field">
          <label style={{ display: "flex", alignItems: "center", gap: "0.4em" }}>
            <input
              type="checkbox"
              style={{ width: "auto" }}
              checked={withUi}
              onChange={(e) => setWithUi(e.target.checked)}
            />
            Also start {UI_LABEL[kind]} (admin UI)
          </label>
        </div>

        {withUi && (
          <div className="form-field">
            <label htmlFor="ui-port">Admin UI port</label>
            <div className="form-row">
              <input
                id="ui-port"
                placeholder={String(Number(port || DEFAULT_PORTS[kind]) + 1)}
                value={uiPort}
                onChange={(e) => setUiPort(e.target.value)}
                style={{ maxWidth: "100px" }}
              />
              <button type="button" className="secondary" onClick={() => handleSuggestPort("ui")}>
                Suggest port
              </button>
            </div>
          </div>
        )}

        {error && <p className="field-error">{error}</p>}

        <div className="form-row">
          <button type="submit" disabled={busy}>
            {busy ? "Creating…" : "Create"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
