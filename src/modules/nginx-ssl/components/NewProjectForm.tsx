import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Mono,
  SegmentedControl,
  Select,
  Spinner,
  Stack,
  StatusPill,
  Switch,
  TextInput,
} from "../../../ui";
import { ExternalLink } from "../../../ui/icons";
import { listDatabases, type DevsuiteContainer } from "../../databases/api";
import { listPhpVersions, scaffoldWizardRun, type PhpFpmVersion } from "../api";
import {
  connectionOption,
  dbNameSlug,
  plannedSteps,
  planSummary,
  splitProjectFolder,
  STEP_LABELS,
  stepState,
  suggestDomain,
  type StepState,
  type WizardPlan,
  type WizardStepReport,
  type WizardTemplate,
} from "../utils";

const SYSTEM_DEFAULT = "";

type WizardStep = 1 | 2 | 3;

const STEP_TITLES: Record<WizardStep, string> = {
  1: "Project",
  2: "Services",
  3: "Review & run",
};

/**
 * Pill per progress row. "pending" is the primitive's spinning variant, so it
 * is used for the step currently running; steps that have not started yet and
 * steps that never ran because an earlier one failed are both neutral, told
 * apart only by their label.
 */
function pillFor(state: StepState): { variant: "ok" | "bad" | "pending" | "neutral"; label: string } {
  switch (state) {
    case "ok":
      return { variant: "ok", label: "done" };
    case "bad":
      return { variant: "bad", label: "failed" };
    case "running":
      return { variant: "pending", label: "running" };
    case "skipped":
      return { variant: "neutral", label: "not run" };
    default:
      return { variant: "neutral", label: "waiting" };
  }
}

export default function NewProjectForm({
  onCreated,
  onCancel,
}: {
  onCreated: () => void;
  onCancel: () => void;
}) {
  const [step, setStep] = useState<WizardStep>(1);

  // Step 1 — project
  const [template, setTemplate] = useState<WizardTemplate>("laravel");
  const [name, setName] = useState("");
  const [parentDir, setParentDir] = useState("");
  const [domain, setDomain] = useState("");
  const [domainTouched, setDomainTouched] = useState(false);
  const [phpVersions, setPhpVersions] = useState<PhpFpmVersion[]>([]);
  const [phpVersion, setPhpVersion] = useState(SYSTEM_DEFAULT);

  // Step 2 — services
  const [containers, setContainers] = useState<DevsuiteContainer[]>([]);
  const [createDatabase, setCreateDatabase] = useState(false);
  const [dbConnection, setDbConnection] = useState("");
  const [dbName, setDbName] = useState("");
  const [dbNameTouched, setDbNameTouched] = useState(false);
  const [writeEnv, setWriteEnv] = useState(true);
  const [runMigrations, setRunMigrations] = useState(true);
  const [editor, setEditor] = useState("");

  // Step 3 — run
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<WizardStepReport[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Both lists are optional niceties: a machine without PHP-FPM or Docker
    // still gets the wizard, just with those controls hidden.
    void listPhpVersions()
      .then(setPhpVersions)
      .catch(() => setPhpVersions([]));
    void listDatabases()
      .then(setContainers)
      .catch(() => setContainers([]));
  }, []);

  const connectionOptions = containers
    .map(connectionOption)
    .filter((o): o is { value: string; label: string } => o !== null);

  function applyName(value: string) {
    setName(value);
    if (!domainTouched) setDomain(suggestDomain(value));
    if (!dbNameTouched) setDbName(dbNameSlug(value));
  }

  function handleTemplateChange(next: WizardTemplate) {
    setTemplate(next);
    setError(null);
    // Migrations only make sense where artisan exists; a blank folder has none.
    setRunMigrations(next !== "blank");
  }

  async function handleBrowseParent() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose a parent folder for the new project",
    });
    if (!selected || Array.isArray(selected)) return;
    setParentDir(selected);
  }

  async function handleBrowseExisting() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Choose the project folder",
    });
    if (!selected || Array.isArray(selected)) return;
    const { parentDir: parent, name: base } = splitProjectFolder(selected);
    setParentDir(parent);
    applyName(base);
  }

  const plan: WizardPlan = {
    name: name.trim(),
    parent_dir: parentDir,
    template,
    domain: domain.trim().toLowerCase(),
    php_version: phpVersion || null,
    create_database: createDatabase,
    db_connection: createDatabase ? dbConnection || null : null,
    db_name: createDatabase ? dbName.trim() || null : null,
    write_env: writeEnv,
    run_migrations: runMigrations,
    open_in_editor: editor.trim() || null,
  };
  const steps = plannedSteps(plan);

  function validateStep(current: WizardStep): string | null {
    if (current === 1) {
      if (!plan.name || !plan.parent_dir) {
        return template === "existing"
          ? "Choose the project folder."
          : "Project name and parent folder are both required.";
      }
      if (!plan.domain) return "A domain is required.";
    }
    if (current === 2 && createDatabase) {
      if (!plan.db_connection) return "Choose a database server, or turn off \"Create database\".";
      if (!plan.db_name) return "A database name is required.";
    }
    return null;
  }

  function goNext() {
    const problem = validateStep(step);
    setError(problem);
    if (problem) return;
    setStep((s) => (s === 1 ? 2 : 3));
  }

  function goBack() {
    setError(null);
    setStep((s) => (s === 3 ? 2 : 1));
  }

  async function handleRun() {
    setError(null);
    setResults(null);
    setRunning(true);
    try {
      setResults(await scaffoldWizardRun(plan));
    } catch (e) {
      // Validation errors from the backend arrive before any step runs.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  const finished = results !== null && !running;
  const allOk = finished && results.every((r) => r.ok);
  const siteLive = finished && results.some((r) => r.step === "site" && r.ok && !r.message.startsWith("skipped"));

  function handleSubmit() {
    if (step === 3) {
      if (finished) onCreated();
      else void handleRun();
    } else {
      goNext();
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <SegmentedControl
          ariaLabel="Wizard step"
          size="sm"
          value={String(step)}
          onChange={(v) => {
            // Only allow jumping backwards; forward moves go through validation.
            const target = Number(v) as WizardStep;
            if (target < step && !running) setStep(target);
          }}
          options={([1, 2, 3] as WizardStep[]).map((s) => ({
            value: String(s),
            label: `${s}. ${STEP_TITLES[s]}`,
          }))}
        />

        {step === 1 && (
          <>
            <Field label="Template">
              <SegmentedControl
                ariaLabel="Project template"
                size="md"
                value={template}
                onChange={handleTemplateChange}
                options={[
                  { value: "laravel", label: "Laravel" },
                  { value: "existing", label: "Existing folder" },
                  { value: "blank", label: "Empty folder" },
                ]}
              />
            </Field>

            {template === "existing" ? (
              <Field label="Project folder" required hint="The folder's name becomes the project name.">
                <Stack gap={3}>
                  <FormRow>
                    <Button variant="secondary" onClick={handleBrowseExisting}>
                      Choose folder…
                    </Button>
                  </FormRow>
                  {parentDir && name && <Mono truncate>{`${parentDir}/${name}`}</Mono>}
                </Stack>
              </Field>
            ) : (
              <>
                <Field label="Project name" htmlFor="wizard-name" required>
                  <TextInput
                    id="wizard-name"
                    placeholder="myapp"
                    value={name}
                    onChange={(e) => applyName(e.target.value)}
                    autoFocus
                  />
                </Field>
                <Field label="Parent folder" required>
                  <Stack gap={3}>
                    <FormRow>
                      <Button variant="secondary" onClick={handleBrowseParent}>
                        Choose folder…
                      </Button>
                    </FormRow>
                    {parentDir && <Mono truncate>{parentDir}</Mono>}
                  </Stack>
                </Field>
              </>
            )}

            <Field
              label="Domain"
              htmlFor="wizard-domain"
              required
              hint="Derived from the project name until you edit it."
            >
              <TextInput
                id="wizard-domain"
                mono
                placeholder="myapp.test"
                value={domain}
                onChange={(e) => {
                  setDomainTouched(true);
                  setDomain(e.target.value);
                }}
              />
            </Field>

            {phpVersions.length > 0 && (
              <Field label="PHP version" htmlFor="wizard-php">
                <Select
                  id="wizard-php"
                  value={phpVersion}
                  onChange={setPhpVersion}
                  options={[
                    { value: SYSTEM_DEFAULT, label: "System default" },
                    ...phpVersions.map((v) => ({ value: v.version, label: `PHP ${v.version}` })),
                  ]}
                />
              </Field>
            )}
          </>
        )}

        {step === 2 && (
          <>
            <Switch
              checked={createDatabase}
              onChange={setCreateDatabase}
              label="Create database"
              description={
                connectionOptions.length === 0
                  ? "No MySQL, PostgreSQL or MongoDB server is set up in Databases yet."
                  : "A new schema on one of the database servers DevsPro manages."
              }
              disabled={connectionOptions.length === 0}
            />
            {createDatabase && (
              <>
                <Field label="Database server" htmlFor="wizard-db-connection" required>
                  <Select
                    id="wizard-db-connection"
                    value={dbConnection}
                    onChange={setDbConnection}
                    placeholder="Choose a server"
                    options={connectionOptions}
                  />
                </Field>
                <Field label="Database name" htmlFor="wizard-db-name" required>
                  <TextInput
                    id="wizard-db-name"
                    mono
                    value={dbName}
                    onChange={(e) => {
                      setDbNameTouched(true);
                      setDbName(e.target.value);
                    }}
                  />
                </Field>
              </>
            )}

            <Switch
              checked={writeEnv}
              onChange={setWriteEnv}
              label="Write .env"
              description="Sets APP_URL, and the DB_* keys when a database is created. Other keys and comments are left as they are."
            />
            <Switch
              checked={runMigrations}
              onChange={setRunMigrations}
              label="Run migrations"
              description="php artisan migrate, generating an app key first if the project has none."
              disabled={template === "blank"}
            />
            <Field
              label="Open in editor"
              htmlFor="wizard-editor"
              hint="Optional command to launch with the project folder, e.g. code or phpstorm."
            >
              <TextInput
                id="wizard-editor"
                mono
                placeholder="code"
                value={editor}
                onChange={(e) => setEditor(e.target.value)}
              />
            </Field>
          </>
        )}

        {step === 3 && (
          <>
            {results === null && !running && (
              <Stack gap={2}>
                {planSummary(plan).map((row) => (
                  <FormRow key={row.label}>
                    <span className="u-muted">{row.label}</span>
                    <Mono truncate>{row.value}</Mono>
                  </FormRow>
                ))}
              </Stack>
            )}

            {(running || results !== null) && (
              <Stack gap={3}>
                {steps.map((id) => {
                  const state = stepState(id, steps, results, running);
                  const pill = pillFor(state);
                  const report = results?.find((r) => r.step === id);
                  return (
                    <Stack key={id} gap={1}>
                      <FormRow>
                        <StatusPill variant={pill.variant} label={pill.label} size="sm" />
                        <span>{STEP_LABELS[id] ?? id}</span>
                      </FormRow>
                      {report && <span className="u-muted">{report.message}</span>}
                    </Stack>
                  );
                })}
              </Stack>
            )}

            {allOk && (
              <Callout tone="success">
                Everything is set up.{" "}
                {siteLive && (
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={ExternalLink}
                    onClick={() => void openUrl(`https://${plan.domain}`)}
                  >
                    Open site
                  </Button>
                )}
              </Callout>
            )}
            {finished && !allOk && (
              <Callout tone="warning">
                A step failed; the ones marked done do not need redoing. Fix the cause and finish
                the rest by hand, or run the wizard again with the completed steps switched off.
              </Callout>
            )}
          </>
        )}

        {error && <Callout tone="danger">{error}</Callout>}

        <FormRow className="form-actions">
          {running && (
            <>
              <Spinner label="Running" />
              <span className="u-muted">This can take a minute — Composer and your admin password may be needed.</span>
            </>
          )}
          {!finished && (
            <Button variant="secondary" onClick={onCancel} disabled={running}>
              Cancel
            </Button>
          )}
          {step > 1 && !finished && (
            <Button variant="secondary" onClick={goBack} disabled={running}>
              Back
            </Button>
          )}
          {step < 3 && (
            <Button type="submit" variant="primary">
              Next
            </Button>
          )}
          {step === 3 && !finished && (
            <Button type="submit" variant="primary" loading={running}>
              Run
            </Button>
          )}
          {finished && (
            <Button type="submit" variant="primary">
              Done
            </Button>
          )}
        </FormRow>
      </Stack>
    </Form>
  );
}
