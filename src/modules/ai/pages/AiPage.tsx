import { Suspense, lazy, useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  Button,
  Callout,
  Card,
  CodeBlock,
  EmptyState,
  Field,
  IconButton,
  Modal,
  Mono,
  Page,
  Select,
  Spinner,
  Stack,
  StatusPill,
  TextInput,
  Toolbar,
  statusOf,
  useResource,
} from "../../../ui";
import { Icon } from "../../../ui/Icon";
import { Bot, Download, ExternalLink, FolderOpen, Pencil, Plus, RefreshCw, Sparkles, Trash2, X } from "../../../ui/icons";
import type { PtyCommand } from "../../ssh/api";
import { AgentStatus, deleteCustomAgent, listAgents } from "../api";
import CustomAgentForm from "../components/CustomAgentForm";
import { baseName, groupAgents, installCommand, launchCommand, rememberProject, sessionTitle } from "../utils";

/* Lazy: xterm.js is a few hundred kB that only a running session needs. */
const Terminal = lazy(() => import("../../../ui/Terminal"));

const PROJECTS_KEY = "devspro.ai.projects";
const PROJECT_KEY = "devspro.ai.project";
const argsKey = (agentId: string) => `devspro.ai.args.${agentId}`;

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* private mode: preferences just don't persist */
  }
}

/** One agent running (or its installer) in its own tab. */
interface AgentSession {
  id: string;
  agentId: string;
  title: string;
  command: PtyCommand;
  kind: "run" | "install";
  exited: boolean;
}

/** The tab in front: the agents catalog, or one running session. */
type View = "agents" | { session: string };

/**
 * The AI agents module, laid out like an editor: a tab strip across the top
 * with the catalog as the first tab and every launched agent as a tab of its
 * own. Sessions stay mounted while another tab is in front, so nothing
 * running is interrupted.
 */
export default function AiPage() {
  const module = useModule("ai");
  const confirmAction = useConfirm();
  const agents = useResource(listAgents);

  const [view, setView] = useState<View>("agents");
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<{ agent?: AgentStatus } | null>(null);

  const [project, setProject] = useState<string | null>(() => readJson<string | null>(PROJECT_KEY, null));
  const [recent, setRecent] = useState<string[]>(() => readJson<string[]>(PROJECTS_KEY, []));

  const list = agents.data ?? [];
  const groups = groupAgents(list);
  const activeSessionId = view === "agents" ? null : view.session;

  function chooseProject(path: string | null) {
    setProject(path);
    write(PROJECT_KEY, path === null ? null : JSON.stringify(path));
    if (path) {
      const next = rememberProject(recent, path);
      setRecent(next);
      write(PROJECTS_KEY, JSON.stringify(next));
    }
  }

  async function browseProject() {
    const picked = await open({ directory: true, multiple: false, title: "Choose a project folder" });
    if (typeof picked === "string") chooseProject(picked);
  }

  function startSession(session: AgentSession) {
    setSessions((prev) => [...prev, session]);
    setView({ session: session.id });
  }

  function launch(agent: AgentStatus) {
    const extra = readJson<string>(argsKey(agent.id), "");
    startSession({
      id: `ai-${crypto.randomUUID()}`,
      agentId: agent.id,
      title: sessionTitle(agent.name, project),
      command: launchCommand(agent, project, extra),
      kind: "run",
      exited: false,
    });
  }

  function install(agent: AgentStatus) {
    if (!agent.install) return;
    startSession({
      id: `ai-${crypto.randomUUID()}`,
      agentId: agent.id,
      title: `Install ${agent.name}`,
      command: installCommand(agent.install_shell, agent.install),
      kind: "install",
      exited: false,
    });
  }

  const onSessionExit = useCallback(
    (id: string) => {
      setSessions((prev) => prev.map((s) => (s.id === id ? { ...s, exited: true } : s)));
      // An installer finishing is what changes an agent's installed state.
      if (sessions.find((s) => s.id === id)?.kind === "install") void agents.refresh();
    },
    [sessions, agents],
  );

  function closeSession(id: string) {
    const index = sessions.findIndex((s) => s.id === id);
    const remaining = sessions.filter((s) => s.id !== id);
    setSessions(remaining);
    if (activeSessionId === id) {
      // Like an editor: the neighbour to the left takes over, else the catalog.
      const next = remaining[Math.max(0, index - 1)];
      setView(next ? { session: next.id } : "agents");
    }
  }

  async function removeCustom(agent: AgentStatus) {
    const ok = await confirmAction(`${agent.name} will be removed from the list. Nothing is uninstalled.`, {
      title: "Remove agent?",
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    try {
      await deleteCustomAgent(agent.id);
      agents.setData(list.filter((a) => a.id !== agent.id));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  function onSaved(saved: AgentStatus) {
    const exists = list.some((a) => a.id === saved.id);
    agents.setData(exists ? list.map((a) => (a.id === saved.id ? saved : a)) : [...list, saved]);
    setForm(null);
  }

  const projectOptions = [
    { value: "", label: "Home directory" },
    ...recent.map((p) => ({ value: p, label: `${baseName(p)}  (${p})` })),
  ];

  return (
    <Page title={module.label} description={module.description} icon={module.icon} fill status={statusOf(agents)}>
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}

      <div className="term-editor-group">
        <div className="term-editor-tabs" role="tablist" aria-label="Agent tabs">
          <button
            type="button"
            role="tab"
            aria-selected={view === "agents"}
            className={`term-editor-tab${view === "agents" ? " term-editor-tab-active" : ""}`}
            onClick={() => setView("agents")}
          >
            <Icon icon={Bot} size="xs" />
            <span className="term-editor-tab-name">Agents</span>
          </button>
          {sessions.map((s) => {
            const active = activeSessionId === s.id;
            return (
              <button
                key={s.id}
                type="button"
                role="tab"
                aria-selected={active}
                className={`term-editor-tab${active ? " term-editor-tab-active" : ""}${s.exited ? " term-pane-exited" : ""}`}
                title={s.exited ? `${s.title} (exited)` : s.title}
                onClick={() => setView({ session: s.id })}
                onAuxClick={(e) => {
                  if (e.button === 1) closeSession(s.id);
                }}
              >
                <Icon icon={s.kind === "install" ? Download : Sparkles} size="xs" />
                <span className="term-editor-tab-name term-pane-title">{s.title}</span>
                <span
                  role="button"
                  tabIndex={-1}
                  aria-label={`Close ${s.title}`}
                  className="term-window-close"
                  onClick={(e) => {
                    e.stopPropagation();
                    closeSession(s.id);
                  }}
                >
                  <Icon icon={X} size="xs" />
                </span>
              </button>
            );
          })}
          <span className="term-editor-tab-launcher">
            <IconButton icon={Plus} label="New session" size="sm" onClick={() => setView("agents")} />
          </span>
        </div>

        <div className="term-tab-body">
          <div className={`term-tab-scroll${view === "agents" ? "" : " term-hidden"}`}>
            <Stack gap={5}>
              <Toolbar
                end={
                  <Stack direction="row" gap={2}>
                    <Button icon={Plus} size="sm" onClick={() => setForm({})}>
                      Custom agent
                    </Button>
                    <Button icon={RefreshCw} size="sm" onClick={() => void agents.refresh()} loading={agents.refreshing}>
                      Refresh
                    </Button>
                  </Stack>
                }
              >
                <Stack direction="row" gap={2} align="center">
                  <span>Project</span>
                  <Select value={project ?? ""} onChange={(v) => chooseProject(v || null)} options={projectOptions} size="sm" />
                  <Button icon={FolderOpen} size="sm" onClick={browseProject}>
                    Browse
                  </Button>
                </Stack>
              </Toolbar>

              {agents.loading ? (
                <Spinner />
              ) : list.length === 0 ? (
                <EmptyState icon={Bot} title="No agents" description="Add a custom agent to get started." size="md" />
              ) : (
                <>
                  {groups.installed.length > 0 && (
                    <div className="term-tab-grid">
                      {groups.installed.map((a) => (
                        <AgentCard
                          key={a.id}
                          agent={a}
                          onLaunch={() => launch(a)}
                          onInstall={() => install(a)}
                          onEdit={() => setForm({ agent: a })}
                          onRemove={() => void removeCustom(a)}
                        />
                      ))}
                    </div>
                  )}
                  {groups.missing.length > 0 && (
                    <Field label="Not installed">
                      <div className="term-tab-grid">
                        {groups.missing.map((a) => (
                          <AgentCard
                            key={a.id}
                            agent={a}
                            onLaunch={() => launch(a)}
                            onInstall={() => install(a)}
                            onEdit={() => setForm({ agent: a })}
                            onRemove={() => void removeCustom(a)}
                          />
                        ))}
                      </div>
                    </Field>
                  )}
                </>
              )}
            </Stack>
          </div>

          {sessions.map((s) => {
            const active = activeSessionId === s.id;
            return (
              <div key={s.id} className={`term-pane${active ? "" : " term-hidden"}`}>
                <Suspense fallback={<Spinner />}>
                  <Terminal
                    id={s.id}
                    command={s.command}
                    active={active}
                    focused={active}
                    onExit={() => onSessionExit(s.id)}
                    onError={setError}
                  />
                </Suspense>
              </div>
            );
          })}
        </div>
      </div>

      {form && (
        <Modal title={form.agent ? `Edit ${form.agent.name}` : "Add a custom agent"} icon={Bot} onClose={() => setForm(null)}>
          <CustomAgentForm agent={form.agent} onSaved={onSaved} onCancel={() => setForm(null)} />
        </Modal>
      )}
    </Page>
  );
}

/** One agent in the catalog: what it is, whether it's here, its extra arguments, and the button. */
function AgentCard({
  agent,
  onLaunch,
  onInstall,
  onEdit,
  onRemove,
}: {
  agent: AgentStatus;
  onLaunch: () => void;
  onInstall: () => void;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const [args, setArgs] = useState(() => readJson<string>(argsKey(agent.id), ""));
  useEffect(() => setArgs(readJson<string>(argsKey(agent.id), "")), [agent.id]);

  function changeArgs(value: string) {
    setArgs(value);
    write(argsKey(agent.id), value ? JSON.stringify(value) : null);
  }

  const commandLine = [agent.command, ...agent.args].join(" ");

  return (
    <Card
      title={agent.name}
      description={agent.vendor}
      headingLevel={3}
      actions={
        <Stack direction="row" gap={1}>
          {agent.homepage && <IconButton icon={ExternalLink} label="Open the project page" size="sm" onClick={() => void openUrl(agent.homepage!)} />}
          {agent.custom && <IconButton icon={Pencil} label="Edit" size="sm" onClick={onEdit} />}
          {agent.custom && <IconButton icon={Trash2} label="Remove" size="sm" onClick={onRemove} />}
        </Stack>
      }
      footer={
        agent.installed ? (
          <Button icon={Sparkles} variant="primary" size="sm" onClick={onLaunch}>
            Launch
          </Button>
        ) : agent.install ? (
          <Button icon={Download} variant="primary" size="sm" onClick={onInstall}>
            Install
          </Button>
        ) : (
          <Button icon={Sparkles} size="sm" onClick={onLaunch}>
            Try to launch anyway
          </Button>
        )
      }
    >
      <Stack gap={3}>
        {agent.description && <p>{agent.description}</p>}
        <Stack direction="row" gap={2} align="center" wrap>
          {agent.installed ? <StatusPill variant="ok" label="Installed" size="sm" /> : <StatusPill variant="neutral" label="Not installed" size="sm" />}
          <Mono variant="chip" title={agent.path ?? undefined}>
            {commandLine}
          </Mono>
        </Stack>
        {!agent.installed && agent.install && <CodeBlock maxHeight="6rem">{agent.install}</CodeBlock>}
        <Field label="Extra arguments" htmlFor={`ai-args-${agent.id}`}>
          <TextInput
            id={`ai-args-${agent.id}`}
            mono
            inputSize="sm"
            placeholder="--resume"
            value={args}
            onChange={(e) => changeArgs(e.target.value)}
          />
        </Field>
      </Stack>
    </Card>
  );
}
