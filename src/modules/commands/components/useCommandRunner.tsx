import { ReactNode, useCallback, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import { Button, Field, Form, FormRow, Modal, Stack, TextInput } from "../../../ui";
import { Code } from "../../../ui/icons";
import { fillPlaceholders, placeholders } from "../library";
import type { LibraryCommand } from "../types";
import "./Commands.css";

/** What a chosen command is for: typed in for editing, run straight away, or copied. */
export type RunMode = "insert" | "run" | "copy";

export interface CommandTargets {
  insert?: (text: string) => void;
  run?: (text: string) => void;
  copy?: (text: string) => void;
}

const LABEL: Record<RunMode, string> = { insert: "Type it in", run: "Run", copy: "Copy" };

/**
 * Takes a chosen command the rest of the way: asks for its `<placeholders>`,
 * and asks again before running one marked dangerous. Typing a command in
 * without running it never asks; the user still has to press Enter.
 */
export function useCommandRunner(targets: CommandTargets): {
  start: (command: LibraryCommand, mode: RunMode) => void;
  element: ReactNode;
} {
  const confirmAction = useConfirm();
  const [prompt, setPrompt] = useState<{ command: LibraryCommand; mode: RunMode } | null>(null);

  const finish = useCallback(
    async (command: LibraryCommand, text: string, mode: RunMode) => {
      if (mode === "run" && command.danger) {
        const ok = await confirmAction(
          <Stack gap={3}>
            <span>{command.description || "This command changes or deletes things."}</span>
            <pre className="cmd-preview">{text}</pre>
          </Stack>,
          { title: `Run "${command.title}"?`, confirmLabel: "Run it", danger: true },
        );
        if (!ok) return;
      }
      targets[mode]?.(text);
    },
    [confirmAction, targets],
  );

  const start = useCallback(
    (command: LibraryCommand, mode: RunMode) => {
      if (placeholders(command.command).length > 0) setPrompt({ command, mode });
      else void finish(command, command.command, mode);
    },
    [finish],
  );

  const element = prompt && (
    <PlaceholderPrompt
      command={prompt.command}
      modes={(Object.keys(LABEL) as RunMode[]).filter((m) => targets[m])}
      preferred={prompt.mode}
      onCancel={() => setPrompt(null)}
      onDone={(text, mode) => {
        setPrompt(null);
        void finish(prompt.command, text, mode);
      }}
    />
  );

  return { start, element };
}

function PlaceholderPrompt({
  command,
  modes,
  preferred,
  onCancel,
  onDone,
}: {
  command: LibraryCommand;
  modes: RunMode[];
  preferred: RunMode;
  onCancel: () => void;
  onDone: (text: string, mode: RunMode) => void;
}) {
  const names = placeholders(command.command);
  const [values, setValues] = useState<Record<string, string>>({});
  const text = fillPlaceholders(command.command, values);
  const complete = names.every((n) => values[n]?.trim());
  // The button Enter presses is the one the user picked; the others sit beside it.
  const others = modes.filter((m) => m !== preferred);

  return (
    <Modal size="md" title={command.title} icon={Code} description={command.description} onClose={onCancel}>
      <Form onSubmit={() => onDone(text, preferred)}>
        <Stack gap={4}>
          {names.map((name, i) => (
            <Field key={name} label={name} htmlFor={`cmd-ph-${name}`}>
              <TextInput
                id={`cmd-ph-${name}`}
                mono
                value={values[name] ?? ""}
                onChange={(e) => setValues((v) => ({ ...v, [name]: e.target.value }))}
                autoFocus={i === 0}
              />
            </Field>
          ))}
          <pre className="cmd-preview">{text}</pre>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onCancel}>
              Cancel
            </Button>
            {others.map((m) => (
              <Button key={m} variant="secondary" onClick={() => onDone(text, m)} disabled={m === "run" && !complete}>
                {LABEL[m]}
              </Button>
            ))}
            <Button type="submit" variant="primary" disabled={preferred === "run" && !complete}>
              {LABEL[preferred]}
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
