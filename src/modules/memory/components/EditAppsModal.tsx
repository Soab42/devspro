import { useId, useState } from "react";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  Field,
  Form,
  FormGrid,
  IconButton,
  Modal,
  ModalFooter,
  Mono,
  SegmentedControl,
  Select,
  Stack,
  TextInput,
  useResource,
} from "../../../ui";
import { Lock, MemoryStick, Pencil, Plus, Trash2 } from "../../../ui/icons";
import { AppDef, AppKind, CATEGORIES, getApps, setApps, Support } from "../api";
import { AppErrors, cleanApp, FULL_SUPPORT, isWindows, KIND_LABEL, uniqueId, validateApp } from "../memory";

const KIND_OPTIONS = (Object.keys(KIND_LABEL) as AppKind[]).map((kind) => ({
  value: kind,
  label: KIND_LABEL[kind],
}));

const blankApp = (): AppDef => ({
  id: "",
  name: "",
  category: "Other",
  kind: "process",
  matcher: "",
  start: null,
  flatpak_id: null,
  service: null,
});

interface Editing {
  /** Position in the list, or null for a new entry. */
  index: number | null;
  app: AppDef;
}

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The catalog: apps the page finds by command line, and how to stop and start
 * them. Changes collect in a draft and are written together on Save, so a
 * half-finished edit never reaches the backend.
 */
export default function EditAppsModal({
  support = FULL_SUPPORT,
  onSaved,
  onClose,
}: {
  /** Leaves out Flatpak where there is none, and names services as this OS does. */
  support?: Support;
  onSaved: (count: number) => void;
  onClose: () => void;
}) {
  const loaded = useResource(getApps);
  const [draft, setDraft] = useState<AppDef[] | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [showErrors, setShowErrors] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const apps = draft ?? loaded.data;

  function edit(index: number | null) {
    setShowErrors(false);
    setEditing({ index, app: index === null ? blankApp() : { ...apps![index] } });
  }

  function apply() {
    if (!editing || !apps) return;
    if (Object.keys(validateApp(editing.app)).length > 0) {
      setShowErrors(true);
      return;
    }
    const app = cleanApp(editing.app);
    if (editing.index === null) {
      setDraft([...apps, { ...app, id: uniqueId(app.name, apps.map((a) => a.id)) }]);
    } else {
      setDraft(apps.map((a, i) => (i === editing.index ? app : a)));
    }
    setEditing(null);
  }

  async function save() {
    if (!draft) return;
    setSaving(true);
    setError(null);
    try {
      await setApps(draft);
      onSaved(draft.length);
    } catch (e) {
      setError(messageOf(e));
      setSaving(false);
    }
  }

  if (editing) {
    return (
      <Modal
        title={editing.index === null ? "Add app" : `Edit ${apps?.[editing.index]?.name ?? "app"}`}
        icon={MemoryStick}
        size="md"
        /* Escape and the close button step back to the list: the draft there
           would be lost otherwise. */
        onClose={() => setEditing(null)}
        footer={
          <ModalFooter>
            <Button variant="secondary" onClick={() => setEditing(null)}>
              Back
            </Button>
            <Button variant="primary" onClick={apply}>
              {editing.index === null ? "Add" : "Done"}
            </Button>
          </ModalFooter>
        }
      >
        <AppForm
          support={support}
          app={editing.app}
          errors={showErrors ? validateApp(editing.app) : {}}
          onChange={(app) => setEditing({ ...editing, app })}
          onSubmit={apply}
        />
      </Modal>
    );
  }

  const columns: Column<AppDef>[] = [
    {
      key: "name",
      header: "App",
      priority: 1,
      sortValue: (a) => a.name.toLowerCase(),
      render: (a) => a.name,
    },
    {
      key: "category",
      header: "Category",
      priority: 2,
      sortValue: (a) => a.category,
      render: (a) => a.category,
    },
    {
      key: "kind",
      header: "Kind",
      priority: 2,
      sortValue: (a) => a.kind,
      render: (a) => (
        <Badge icon={a.kind === "service" ? Lock : undefined}>{KIND_LABEL[a.kind]}</Badge>
      ),
    },
    {
      key: "matcher",
      header: "Command line contains",
      priority: 3,
      render: (a) => <Mono truncate>{a.matcher}</Mono>,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (a) => {
        const index = apps!.indexOf(a);
        return (
          <ActionGroup>
            <IconButton icon={Pencil} label={`Edit ${a.name}`} onClick={() => edit(index)} />
            <IconButton
              icon={Trash2}
              label={`Remove ${a.name}`}
              onClick={() => setDraft(apps!.filter((_, i) => i !== index))}
            />
          </ActionGroup>
        );
      },
    },
  ];

  return (
    <Modal
      title="Edit apps"
      description="The apps this page looks for, and how to stop and start them. Anything else you run shows up as detected."
      icon={MemoryStick}
      size="lg"
      onClose={onClose}
      footer={
        <ModalFooter hint={draft ? "Changes aren't saved yet." : undefined}>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={saving} disabled={!draft} onClick={() => void save()}>
            Save
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={4}>
        {(error ?? loaded.error) && (
          <Callout tone="danger" onDismiss={error ? () => setError(null) : undefined}>
            {error ?? loaded.error}
          </Callout>
        )}
        <Stack direction="row" justify="end">
          <Button icon={Plus} disabled={!apps} onClick={() => edit(null)}>
            Add app
          </Button>
        </Stack>
        <DataTable
          columns={columns}
          rows={apps ?? []}
          rowKey={(a) => a.id}
          loading={!apps && !loaded.error}
          defaultSort={{ key: "name", dir: "asc" }}
          caption="Apps the Memory page looks for"
          minWidth={520}
          empty={{
            icon: MemoryStick,
            title: "No apps yet",
            description: "Running apps still show up as detected.",
          }}
        />
      </Stack>
    </Modal>
  );
}

function AppForm({
  support,
  app,
  errors,
  onChange,
  onSubmit,
}: {
  support: Support;
  app: AppDef;
  errors: AppErrors;
  onChange: (app: AppDef) => void;
  onSubmit: () => void;
}) {
  const ids = {
    name: useId(),
    category: useId(),
    matcher: useId(),
    extra: useId(),
  };
  const set = <K extends keyof AppDef>(key: K, value: AppDef[K]) => onChange({ ...app, [key]: value });
  const categories = (CATEGORIES as readonly string[]).includes(app.category)
    ? CATEGORIES
    : [...CATEGORIES, app.category];
  const windows = isWindows(support);
  // An entry that is already a Flatpak keeps the option, so its kind still shows.
  const kinds = KIND_OPTIONS.filter((k) => k.value !== "flatpak" || support.flatpak || app.kind === "flatpak");

  return (
    <Form onSubmit={onSubmit}>
      <Stack gap={4}>
        <FormGrid columns={2}>
          <Field label="Name" htmlFor={ids.name} error={errors.name} required>
            <TextInput
              id={ids.name}
              autoFocus
              value={app.name}
              invalid={Boolean(errors.name)}
              onChange={(e) => set("name", e.target.value)}
            />
          </Field>
          <Field label="Category" htmlFor={ids.category}>
            <Select
              id={ids.category}
              aria-label="Category"
              value={app.category}
              onChange={(v) => set("category", v)}
              options={categories.map((c) => ({ value: c, label: c }))}
            />
          </Field>
        </FormGrid>

        <Field label="Kind">
          <SegmentedControl
            ariaLabel="Kind"
            size="md"
            value={app.kind}
            onChange={(kind) => set("kind", kind)}
            options={kinds}
          />
        </Field>

        <Field
          label="Command line contains"
          htmlFor={ids.matcher}
          hint="Every process whose command line includes this counts as this app."
          error={errors.matcher}
          required
        >
          <TextInput
            id={ids.matcher}
            mono
            placeholder={windows ? "\\Google\\Chrome\\Application\\chrome.exe" : "/opt/google/chrome/chrome"}
            value={app.matcher}
            invalid={Boolean(errors.matcher)}
            onChange={(e) => set("matcher", e.target.value)}
          />
        </Field>

        {app.kind === "process" && (
          <Field
            label="Start command"
            htmlFor={ids.extra}
            hint={`Runs with ${windows ? "cmd" : "sh"}. Leave it empty if the app can't be started from here.`}
          >
            <TextInput
              id={ids.extra}
              mono
              placeholder="google-chrome"
              value={app.start ?? ""}
              onChange={(e) => set("start", e.target.value)}
            />
          </Field>
        )}
        {app.kind === "flatpak" && (
          <Field
            label="Flatpak ID"
            htmlFor={ids.extra}
            hint="Stopped with flatpak kill and started with flatpak run."
            error={errors.flatpak_id}
            required
          >
            <TextInput
              id={ids.extra}
              mono
              placeholder="org.gimp.GIMP"
              value={app.flatpak_id ?? ""}
              invalid={Boolean(errors.flatpak_id)}
              onChange={(e) => set("flatpak_id", e.target.value)}
            />
          </Field>
        )}
        {app.kind === "service" && (
          <Field
            label={windows ? "Service name" : "systemd unit"}
            htmlFor={ids.extra}
            hint="Starting and stopping it asks for your admin password."
            error={errors.service}
            required
          >
            <TextInput
              id={ids.extra}
              mono
              placeholder={windows ? "MySQL80" : "nginx"}
              value={app.service ?? ""}
              invalid={Boolean(errors.service)}
              onChange={(e) => set("service", e.target.value)}
            />
          </Field>
        )}
      </Stack>
    </Form>
  );
}
