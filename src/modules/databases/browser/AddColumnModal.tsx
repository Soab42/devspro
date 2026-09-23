import { useState } from "react";
import {
  Button,
  Callout,
  Field,
  Form,
  FormRow,
  Modal,
  ModalFooter,
  SelectOption,
  Select,
  Stack,
  Switch,
  TextInput,
} from "../../../ui";
import { Plus } from "../../../ui/icons";
import { addColumn } from "../api";

export interface AddColumnModalProps {
  connection: string;
  database: string;
  table: string;
  engine: "mysql" | "postgres";
  onClose: () => void;
  onAdded: (message: string) => void;
}

/** Common types, per engine, so the field is a pick rather than a spelling test. */
const TYPES: Record<"mysql" | "postgres", SelectOption[]> = {
  mysql: [
    { value: "VARCHAR(255)", label: "VARCHAR(255)" },
    { value: "TEXT", label: "TEXT" },
    { value: "INT", label: "INT" },
    { value: "BIGINT", label: "BIGINT" },
    { value: "DECIMAL(10, 2)", label: "DECIMAL(10, 2)" },
    { value: "TINYINT(1)", label: "BOOLEAN (TINYINT(1))" },
    { value: "DATETIME", label: "DATETIME" },
    { value: "JSON", label: "JSON" },
  ],
  postgres: [
    { value: "varchar(255)", label: "varchar(255)" },
    { value: "text", label: "text" },
    { value: "integer", label: "integer" },
    { value: "bigint", label: "bigint" },
    { value: "numeric(10, 2)", label: "numeric(10, 2)" },
    { value: "boolean", label: "boolean" },
    { value: "timestamptz", label: "timestamptz" },
    { value: "jsonb", label: "jsonb" },
  ],
};

/**
 * The one piece of DDL the browser offers.
 *
 * "I need somewhere to put this" is the schema change people actually make
 * while looking at data; anything further — renaming, dropping, changing a
 * type — belongs in a migration that can be reviewed and replayed, not in a
 * grid.
 */
export default function AddColumnModal({
  connection,
  database,
  table,
  engine,
  onClose,
  onAdded,
}: AddColumnModalProps) {
  const [name, setName] = useState("");
  const [dataType, setDataType] = useState(TYPES[engine][0].value);
  const [custom, setCustom] = useState("");
  const [nullable, setNullable] = useState(true);
  const [defaultValue, setDefaultValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const type = custom.trim() || dataType;

  async function handleSubmit() {
    if (!name.trim()) {
      setError("A column needs a name.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      onAdded(
        await addColumn({
          connection,
          database,
          table,
          name: name.trim(),
          dataType: type,
          nullable,
          defaultValue,
        }),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      size="md"
      title={`Add a column to "${table}"`}
      icon={Plus}
      onClose={onClose}
      footer={
        <ModalFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={handleSubmit}>
            Add column
          </Button>
        </ModalFooter>
      }
    >
      <Form onSubmit={handleSubmit}>
        <Stack gap={5}>
          <FormRow align="end">
            <Field label="Name" htmlFor="col-name" required>
              <TextInput
                id="col-name"
                mono
                placeholder="e.g. archived_at"
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
              />
            </Field>
            <Field label="Type" htmlFor="col-type">
              <Select
                id="col-type"
                value={dataType}
                options={TYPES[engine]}
                disabled={custom.trim() !== ""}
                onChange={setDataType}
              />
            </Field>
          </FormRow>

          <Field
            label="Or type it yourself"
            htmlFor="col-custom"
            hint="Overrides the picker. Anything the engine accepts."
          >
            <TextInput
              id="col-custom"
              mono
              placeholder={dataType}
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
            />
          </Field>

          <Field
            label="Default"
            htmlFor="col-default"
            hint="Optional. Leave blank for no default."
          >
            <TextInput
              id="col-default"
              mono
              value={defaultValue}
              onChange={(e) => setDefaultValue(e.target.value)}
            />
          </Field>

          <Switch
            checked={nullable}
            onChange={setNullable}
            label="Allow NULL"
            description="A NOT NULL column needs a default if the table already has rows — the engine will say so."
          />

          {error && <Callout tone="danger">{error}</Callout>}
        </Stack>
      </Form>
    </Modal>
  );
}
