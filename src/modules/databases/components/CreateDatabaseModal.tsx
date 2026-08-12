import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Modal,
  ModalFooter,
  Mono,
  SelectOption,
  Select,
  Stack,
  Switch,
  TextInput,
} from "../../../ui";
import { CircleCheck, Database, WandSparkles } from "../../../ui/icons";
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

export default function CreateDatabaseModal({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
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

  async function handleSubmit() {
    setError(null);
    const portNum = Number(port);
    if (!name.trim() || !portNum) {
      setError("Name and port are both required.");
      return;
    }
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
        uiPort: withUi ? Number(uiPort) || null : null,
      });
      setResult(created);
      onCreated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  if (result) {
    return (
      <Modal
        size="sm"
        title="Database created"
        icon={CircleCheck}
        onClose={onClose}
        footer={
          <ModalFooter>
            <Button variant="primary" onClick={onClose}>
              Done
            </Button>
          </ModalFooter>
        }
      >
        <Stack gap={5}>
          <Callout tone="success">{result.message}</Callout>

          {result.username && (
            <Field label="Username">
              <Mono variant="chip" copyable>
                {result.username}
              </Mono>
            </Field>
          )}

          <Field label="Password">
            <Mono variant="chip" copyable>
              {result.password}
            </Mono>
          </Field>

          <Field label="Connect on">
            <Mono variant="chip" copyable>
              {`localhost:${result.db_port}`}
            </Mono>
          </Field>

          {result.ui_port && (
            <Field label="Admin UI">
              <Mono variant="chip" copyable>
                {`localhost:${result.ui_port}`}
              </Mono>
            </Field>
          )}

          <Callout tone="info">
            This password is saved in DevsPro's compose file — use{" "}
            <strong>Reveal password</strong> later if you lose it.
          </Callout>
        </Stack>
      </Modal>
    );
  }

  return (
    <Modal
      size="md"
      title="Create a database"
      icon={Database}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSubmit}>
            Create
          </Button>
        </ModalFooter>
      }
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={5}>
          <FormRow align="end">
            <Field label="Type" htmlFor="db-kind">
              <Select
                id="db-kind"
                value={kind}
                onChange={handleKindChange}
                options={KIND_OPTIONS}
              />
            </Field>
            <Field label="Version" htmlFor="db-version">
              <Select
                id="db-version"
                value={version}
                onChange={setVersion}
                options={VERSION_OPTIONS[kind]}
              />
            </Field>
          </FormRow>

          <Field label="Name" htmlFor="db-name" required>
            <TextInput
              id="db-name"
              placeholder="e.g. myapp_db"
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoFocus
            />
          </Field>

          <Field label="Port" htmlFor="db-port" required>
            <FormRow>
              <TextInput
                id="db-port"
                mono
                inputMode="numeric"
                className="field-input-narrow"
                placeholder={String(DEFAULT_PORTS[kind])}
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />
              <Button
                variant="secondary"
                icon={WandSparkles}
                onClick={() => handleSuggestPort("db")}
              >
                Suggest
              </Button>
            </FormRow>
          </Field>

          {kind !== "redis" && (
            <Field label="Username" htmlFor="db-username">
              <TextInput
                id="db-username"
                mono
                placeholder={DEFAULT_USERNAME[kind]}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </Field>
          )}

          <Field
            label="Password"
            htmlFor="db-password"
            hint="Leave blank to auto-generate one."
          >
            <FormRow>
              <TextInput
                id="db-password"
                mono
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="u-flex-1"
              />
              <Button
                variant="secondary"
                icon={WandSparkles}
                onClick={() => setPassword(generatePassword())}
              >
                Generate
              </Button>
            </FormRow>
          </Field>

          <Switch
            checked={withUi}
            onChange={setWithUi}
            label={`Also start ${UI_LABEL[kind]}`}
            description="A browser-based admin UI for this database."
          />

          {withUi && (
            <Field label="Admin UI port" htmlFor="ui-port">
              <FormRow>
                <TextInput
                  id="ui-port"
                  mono
                  inputMode="numeric"
                  className="field-input-narrow"
                  placeholder={String(Number(port || DEFAULT_PORTS[kind]) + 1)}
                  value={uiPort}
                  onChange={(e) => setUiPort(e.target.value)}
                />
                <Button
                  variant="secondary"
                  icon={WandSparkles}
                  onClick={() => handleSuggestPort("ui")}
                >
                  Suggest
                </Button>
              </FormRow>
            </Field>
          )}

          {error && <Callout tone="danger">{error}</Callout>}
        </Stack>
      </Form>
    </Modal>
  );
}
