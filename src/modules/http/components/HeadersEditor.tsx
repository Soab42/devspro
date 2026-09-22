import { Button, Field, FormRow, IconButton, Stack, TextInput } from "../../../ui";
import { Plus, Trash2 } from "../../../ui/icons";
import { HeaderRow, newHeaderRow } from "../utils";

interface Props {
  rows: HeaderRow[];
  onChange: (rows: HeaderRow[]) => void;
  disabled?: boolean;
}

/**
 * Key/value rows. The last row is always blank so there is somewhere to type
 * the next header without a click first; removing the only row leaves one
 * blank row rather than an empty editor with no input to focus.
 */
export default function HeadersEditor({ rows, onChange, disabled }: Props) {
  function update(id: string, patch: Partial<Pick<HeaderRow, "key" | "value">>) {
    onChange(rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  }

  function remove(id: string) {
    const next = rows.filter((r) => r.id !== id);
    onChange(next.length === 0 ? [newHeaderRow()] : next);
  }

  return (
    <Stack gap={2}>
      {rows.map((row, index) => (
        <FormRow key={row.id}>
          <Field>
            <TextInput
              mono
              inputSize="sm"
              placeholder="Header"
              aria-label={`Header ${index + 1} name`}
              value={row.key}
              disabled={disabled}
              onChange={(e) => update(row.id, { key: e.target.value })}
            />
          </Field>
          <Field>
            <TextInput
              mono
              inputSize="sm"
              placeholder="Value"
              aria-label={`Header ${index + 1} value`}
              value={row.value}
              disabled={disabled}
              onChange={(e) => update(row.id, { value: e.target.value })}
            />
          </Field>
          <IconButton
            icon={Trash2}
            label="Remove header"
            size="sm"
            variant="ghost"
            disabled={disabled}
            onClick={() => remove(row.id)}
          />
        </FormRow>
      ))}
      <div>
        <Button
          variant="ghost"
          size="sm"
          icon={Plus}
          disabled={disabled}
          onClick={() => onChange([...rows, newHeaderRow()])}
        >
          Add header
        </Button>
      </div>
    </Stack>
  );
}
