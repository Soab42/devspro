import { useState } from "react";
import { Button, Callout, Checkbox, Field, Form, FormRow, Modal, Stack, TextInput } from "../../../ui";
import { Plus } from "../../../ui/icons";
import { errorMessage } from "../../ssh/utils";
import type { PersonalCommand } from "../types";
import "./Commands.css";

export interface CommandDraft {
  /** Set when editing one of the profile's own commands. */
  id?: string;
  title: string;
  command: string;
  description: string;
  category: string;
  danger: boolean;
  createdAt?: string;
}

export const emptyDraft: CommandDraft = { title: "", command: "", description: "", category: "", danger: false };

/** How many existing categories to offer as one-click choices. */
const SUGGESTED = 12;

/**
 * Adds a personal command, edits one, or saves a copy of a global one to
 * change. Categories are free text; the ones already in use are offered so
 * the same name isn't typed two ways.
 */
export function CommandForm({
  initial,
  categories,
  onSave,
  onClose,
}: {
  initial: CommandDraft;
  categories: string[];
  onSave: (command: PersonalCommand) => Promise<void>;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = <K extends keyof CommandDraft>(key: K, value: CommandDraft[K]) => setDraft((d) => ({ ...d, [key]: value }));

  const typed = draft.category.trim().toLowerCase();
  const suggestions = categories
    .filter((c) => c.toLowerCase() !== typed && (!typed || c.toLowerCase().includes(typed)))
    .slice(0, SUGGESTED);

  async function submit() {
    const title = draft.title.trim();
    const command = draft.command.trim();
    if (!title || !command) return setError("A command needs a title and the command itself.");
    if (/[\r\n]/.test(command)) return setError("Keep the command on one line; join steps with && instead.");
    setBusy(true);
    setError(null);
    try {
      await onSave({
        id: draft.id ?? crypto.randomUUID(),
        title,
        command,
        description: draft.description.trim(),
        category: draft.category.trim() || "Uncategorised",
        danger: draft.danger,
        createdAt: draft.createdAt ?? new Date().toISOString(),
      });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  }

  return (
    <Modal size="md" title={draft.id ? "Edit command" : "New command"} icon={Plus} onClose={onClose}>
      <Form onSubmit={() => void submit()}>
        <Stack gap={5}>
          <Field label="Title" htmlFor="cmd-title" required>
            <TextInput
              id="cmd-title"
              placeholder="Restart the API"
              value={draft.title}
              onChange={(e) => set("title", e.target.value)}
              autoFocus
            />
          </Field>
          <Field
            label="Command"
            htmlFor="cmd-command"
            required
            hint="Mark what changes each time as <name>, for example <branch>; you are asked for it before it is typed."
          >
            <TextInput
              id="cmd-command"
              mono
              placeholder="sudo systemctl restart <service>"
              value={draft.command}
              onChange={(e) => set("command", e.target.value)}
            />
          </Field>
          <Field label="Description" htmlFor="cmd-description">
            <TextInput
              id="cmd-description"
              placeholder="What it does, and anything to watch out for"
              value={draft.description}
              onChange={(e) => set("description", e.target.value)}
            />
          </Field>
          <Field label="Category" htmlFor="cmd-category">
            <Stack gap={2}>
              <TextInput
                id="cmd-category"
                placeholder="Deploy"
                value={draft.category}
                onChange={(e) => set("category", e.target.value)}
              />
              {suggestions.length > 0 && (
                <div className="cmd-category-chips">
                  {suggestions.map((c) => (
                    <button key={c} type="button" className="cmd-category-chip" onClick={() => set("category", c)}>
                      {c}
                    </button>
                  ))}
                </div>
              )}
            </Stack>
          </Field>
          <Checkbox
            label="Ask before running (it changes or deletes things)"
            checked={draft.danger}
            onChange={(checked) => set("danger", checked)}
          />
          {error && <Callout tone="danger">{error}</Callout>}
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" loading={busy}>
              {draft.id ? "Save changes" : "Add command"}
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
