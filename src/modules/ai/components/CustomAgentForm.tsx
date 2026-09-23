import { useState } from "react";
import { Button, Callout, Field, Form, Stack, TextArea, TextInput } from "../../../ui";
import { AgentStatus, saveCustomAgent } from "../api";
import { splitArgs } from "../utils";

/** Add or edit an agent that isn't in the catalog: a name and the command that starts it. */
export default function CustomAgentForm({
  agent,
  onSaved,
  onCancel,
}: {
  agent?: AgentStatus;
  onSaved: (saved: AgentStatus) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(agent?.name ?? "");
  const [command, setCommand] = useState(agent?.command ?? "");
  const [args, setArgs] = useState(agent?.args.join(" ") ?? "");
  const [description, setDescription] = useState(agent?.description ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function handleSubmit() {
    if (!name.trim()) {
      setError("Give the agent a name.");
      return;
    }
    if (!command.trim()) {
      setError("Enter the command that starts it.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      onSaved(
        await saveCustomAgent(agent?.id ?? null, {
          name: name.trim(),
          command: command.trim(),
          args: splitArgs(args),
          description: description.trim(),
        }),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Form onSubmit={handleSubmit}>
      <Stack gap={5}>
        <Field label="Name" htmlFor="ai-custom-name" required>
          <TextInput id="ai-custom-name" placeholder="My agent" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>
        <Field
          label="Command"
          htmlFor="ai-custom-command"
          required
          description="The program to run, as you would type it in a terminal: a name on your PATH or a full path."
        >
          <TextInput id="ai-custom-command" mono placeholder="puku" value={command} onChange={(e) => setCommand(e.target.value)} />
        </Field>
        <Field label="Arguments" htmlFor="ai-custom-args" description="Always passed to the command. Quote an argument that contains spaces.">
          <TextInput id="ai-custom-args" mono placeholder="--model sonnet" value={args} onChange={(e) => setArgs(e.target.value)} />
        </Field>
        <Field label="Description" htmlFor="ai-custom-description">
          <TextArea id="ai-custom-description" rows={2} value={description} onChange={(e) => setDescription(e.target.value)} />
        </Field>
        {error && <Callout tone="danger">{error}</Callout>}
        <Stack direction="row" gap={2} justify="flex-end">
          <Button onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" loading={busy}>
            {agent ? "Save" : "Add agent"}
          </Button>
        </Stack>
      </Stack>
    </Form>
  );
}
