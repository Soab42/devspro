import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useModule } from "../../../app/ModuleRegistry";
import { POPOUT_ACTIONS_SLOT, POPOUT_TABS_SLOT, currentPopoutModuleId, isTerminalWindow } from "../../../app/popout";
import { usePopoutHandoff } from "../../../app/popoutState";
import { Badge, Button, Callout, Page, useResource } from "../../../ui";
import { Plus } from "../../../ui/icons";
import { listHosts } from "../../ssh/api";
import TerminalsTab, { WorkspaceHandle } from "../components/TerminalsTab";

export default function TerminalPage() {
  // Any popped-out window — one pane, or the whole module — is a window of
  // terminals with its controls in the top bar, no page header.
  return isTerminalWindow() || currentPopoutModuleId() === "terminal" ? <TerminalWindowPage /> : <TerminalModulePage />;
}

/**
 * A popped-out terminal window. Popped from a pane, it is seeded with the
 * session the main window started: the session id, its command and its
 * title travel in the URL, the process itself is adopted from Rust. Popped
 * as a module, it starts empty. Either way the window strip sits beside
 * the title in the top bar, like browser tabs, and the buttons before the
 * theme toggle.
 */
function TerminalWindowPage() {
  const [params, setParams] = useSearchParams();
  const hosts = useResource(listHosts);
  const [error, setError] = useState<string | null>(null);
  const ptyId = params.get("pty");
  const host = params.get("host");
  const title = params.get("title") || "Terminal";
  const solo = isTerminalWindow();

  useEffect(() => {
    if (solo) document.title = `${title} \u2014 DevsPro`;
  }, [solo, title]);

  // Fixed for the window's life: the pane it was opened for.
  const [initial] = useState(() =>
    solo && ptyId
      ? {
          session: { id: ptyId, title, command: host ? { ssh: host } : ("local" as const), exited: false, cwd: null },
          attached: true as const,
        }
      : undefined,
  );
  // The popout shell's top bar has slots for the strip and the controls.
  const [actionsSlot, setActionsSlot] = useState<Element | null>(null);
  const [tabsSlot, setTabsSlot] = useState<Element | null>(null);
  useEffect(() => {
    setActionsSlot(document.getElementById(POPOUT_ACTIONS_SLOT));
    setTabsSlot(document.getElementById(POPOUT_TABS_SLOT));
  }, []);

  const workspace = useRef<WorkspaceHandle | null>(null);
  const onReady = useCallback((handle: WorkspaceHandle) => {
    workspace.current = handle;
  }, []);
  const local = params.get("local");
  useEffect(() => {
    if (solo || (!host && !local)) return;
    workspace.current?.openWindow(host ? { ssh: host } : "local");
    setParams({}, { replace: true });
  }, [solo, host, local, setParams]);

  if (solo && !ptyId) return <Callout tone="danger">This window has no terminal to show.</Callout>;

  return (
    <div className="term-window-page">
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <TerminalsTab
        hosts={hosts.data ?? []}
        hidden={false}
        initial={initial}
        solo={solo}
        actionsPortal={actionsSlot}
        tabsPortal={tabsSlot}
        onReady={onReady}
        onError={setError}
      />
    </div>
  );
}

/**
 * The Terminal module. Opens with `?local=1` for a fresh local shell or
 * `?host=<id>` for a saved host (the SSH module's host rows link here);
 * either is consumed once and dropped from the URL, so a reload doesn't
 * open another.
 */
function TerminalModulePage() {
  const module = useModule("terminal");
  const hosts = useResource(listHosts);
  const [params, setParams] = useSearchParams();
  const [error, setError] = useState<string | null>(null);
  const [count, setCount] = useState(0);
  // The workspace's buttons live in the page header, beside "Local terminal",
  // and its window strip beside the title.
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);
  const [tabsSlot, setTabsSlot] = useState<HTMLDivElement | null>(null);

  const workspace = useRef<WorkspaceHandle | null>(null);
  const onReady = useCallback((handle: WorkspaceHandle) => {
    workspace.current = handle;
  }, []);

  const host = params.get("host");
  const local = params.get("local");
  useEffect(() => {
    if (!host && !local) return;
    workspace.current?.openWindow(host ? { ssh: host } : "local");
    setParams({}, { replace: true });
  }, [host, local, setParams]);

  // The top bar's "new window" button takes the terminal you're in with it,
  // the same as the pane's own pop-out button; with nothing open it falls
  // back to an empty terminal window.
  usePopoutHandoff(() => ({
    open: async () => (await workspace.current?.popOutFocused()) ?? false,
  }));

  return (
    <Page
      title={module.label}
      icon={module.icon}
      fill
      meta={
        <>
          {count > 0 && <Badge tone="accent">{count} open</Badge>}
          <div ref={setTabsSlot} className="term-page-tabs-slot" />
        </>
      }
      actions={
        <>
          <div ref={setSlot} className="term-page-actions-slot" />
          <Button variant="primary" icon={Plus} onClick={() => workspace.current?.openWindow("local")}>
            Local terminal
          </Button>
        </>
      }
    >
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <TerminalsTab
        hosts={hosts.data ?? []}
        hidden={false}
        actionsPortal={slot}
        tabsPortal={tabsSlot}
        onReady={onReady}
        onCountChange={setCount}
        onError={setError}
      />
    </Page>
  );
}
