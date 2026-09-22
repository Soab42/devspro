import { Suspense, lazy } from "react";
import { Button, EmptyState, IconButton, Spinner, Tabs } from "../../../ui";
import { Plus, SquareTerminal, X } from "../../../ui/icons";
import { TerminalWorkspace } from "../../../ui/Terminal";
import type { PtyCommand } from "../api";

/* Lazy: xterm.js is a few hundred kB that only this tab needs. The wrapper
   (TerminalWorkspace) is a plain layout component and is imported eagerly. */
const Terminal = lazy(() => import("../../../ui/Terminal"));

export interface TerminalSession {
  id: string;
  title: string;
  command: PtyCommand;
  exited: boolean;
}

export default function TerminalsTab({
  sessions,
  activeId,
  hidden,
  onActivate,
  onClose,
  onExited,
  onError,
  onNewLocal,
}: {
  sessions: TerminalSession[];
  hidden: boolean;
  activeId: string | null;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onExited: (id: string) => void;
  onError: (message: string) => void;
  onNewLocal: () => void;
}) {
  const active = sessions.find((s) => s.id === activeId) ?? null;

  return (
    <TerminalWorkspace
      hidden={hidden}
      tabs={
        sessions.length > 0 && (
          <Tabs
            ariaLabel="Open terminals"
            tabs={sessions.map((s) => ({ id: s.id, label: s.exited ? `${s.title} (exited)` : s.title }))}
            active={activeId ?? sessions[0].id}
            onChange={onActivate}
          />
        )
      }
      actions={
        <>
          {active && (
            <IconButton icon={X} label="Close terminal" onClick={() => onClose(active.id)} />
          )}
          <Button variant="secondary" size="sm" icon={Plus} onClick={onNewLocal}>
            Local terminal
          </Button>
        </>
      }
      empty={
        sessions.length === 0 && (
          <EmptyState
            icon={SquareTerminal}
            size="md"
            title="No open terminals"
            description="Open one from a saved host, or start a local shell."
            action={
              <Button variant="primary" icon={Plus} onClick={onNewLocal}>
                Local terminal
              </Button>
            }
          />
        )
      }
    >
      <Suspense fallback={<Spinner />}>
        {sessions.map((s) => (
          <Terminal
            key={s.id}
            id={s.id}
            command={s.command}
            active={s.id === activeId}
            onExit={() => onExited(s.id)}
            onError={onError}
          />
        ))}
      </Suspense>
    </TerminalWorkspace>
  );
}
