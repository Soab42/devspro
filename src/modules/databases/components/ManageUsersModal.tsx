import { useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Button,
  Callout,
  Column,
  DataTable,
  Field,
  Form,
  FormRow,
  IconButton,
  Modal,
  ModalFooter,
  Section,
  Stack,
  TextInput,
  useResource,
} from "../../../ui";
import { KeyRound, Trash2, Users, WandSparkles } from "../../../ui/icons";
import { createDatabaseUser, dropDatabaseUser, listDatabaseUsers } from "../api";
import { generatePassword } from "../utils";
import ChangePasswordModal from "./ChangePasswordModal";

export default function ManageUsersModal({
  base,
  onClose,
}: {
  base: string;
  onClose: () => void;
}) {
  const confirmAction = useConfirm();
  const users = useResource(() => listDatabaseUsers(base));

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [busyUser, setBusyUser] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [changingPasswordFor, setChangingPasswordFor] = useState<string | null>(null);

  async function handleCreate() {
    if (!username.trim() || !password.trim()) {
      setError("Username and password are both required.");
      return;
    }
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      setMessage(await createDatabaseUser(base, username.trim(), password.trim()));
      setUsername("");
      setPassword("");
      await users.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDrop(user: string) {
    const ok = await confirmAction(
      `Anything still connecting as "${user}" will stop working.`,
      { title: "Remove database user?", confirmLabel: "Remove user", danger: true },
    );
    if (!ok) return;

    setBusyUser(user);
    setError(null);
    try {
      setMessage(await dropDatabaseUser(base, user));
      await users.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyUser(null);
    }
  }

  const columns: Column<string>[] = [
    {
      key: "name",
      header: "Username",
      priority: 1,
      mono: true,
      sortValue: (u) => u,
      render: (u) => u,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (u) => (
        <ActionGroup>
          <IconButton
            icon={KeyRound}
            label="Change password"
            onClick={() => setChangingPasswordFor(u)}
          />
          <IconButton
            icon={Trash2}
            label="Remove user"
            variant="danger"
            loading={busyUser === u}
            onClick={() => handleDrop(u)}
          />
        </ActionGroup>
      ),
    },
  ];

  return (
    <Modal
      size="lg"
      title={`Users — ${base}`}
      icon={Users}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
        </ModalFooter>
      }
    >
      <Stack gap={5}>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}
        {message && (
          <Callout tone="success" onDismiss={() => setMessage(null)}>
            {message}
          </Callout>
        )}

        <Section title="Existing users" padding="none" headingLevel={3}>
          <DataTable
            columns={columns}
            rows={users.data ?? []}
            rowKey={(u) => u}
            loading={users.loading}
            minWidth={320}
            caption={`Database users for ${base}`}
            empty={{
              icon: Users,
              title: "No extra users yet",
              description:
                'The admin account is managed from "Reveal password" on the database row.',
            }}
          />
        </Section>

        <Section title="Add a user" headingLevel={3}>
          <Form onSubmit={handleCreate}>
            <Stack gap={4}>
              <Field label="Username" htmlFor="new-user-name" required>
                <TextInput
                  id="new-user-name"
                  mono
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                />
              </Field>

              <Field label="Password" htmlFor="new-user-password" required>
                <FormRow>
                  <TextInput
                    id="new-user-password"
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

              <FormRow className="form-actions">
                <Button type="submit" variant="primary" loading={busy}>
                  Add user
                </Button>
              </FormRow>
            </Stack>
          </Form>
        </Section>
      </Stack>

      {changingPasswordFor && (
        <ChangePasswordModal
          base={base}
          username={changingPasswordFor}
          onClose={() => setChangingPasswordFor(null)}
        />
      )}
    </Modal>
  );
}
