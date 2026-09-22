import { useState } from "react";
import { Button, Field, Form, FormRow, Modal, Stack, TextInput } from "../../../ui";

export default function NameDialog({
  title,
  initial,
  confirmLabel,
  onSubmit,
  onClose,
}: {
  title: string;
  initial: string;
  confirmLabel: string;
  onSubmit: (value: string) => void;
  onClose: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <Modal size="sm" title={title} onClose={onClose}>
      <Form onSubmit={() => onSubmit(value)}>
        <Stack gap={5}>
          <Field label="Name" htmlFor="fm-name">
            <TextInput id="fm-name" mono value={value} onChange={(e) => setValue(e.target.value)} autoFocus />
          </Field>
          <FormRow className="form-actions">
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={!value.trim()}>
              {confirmLabel}
            </Button>
          </FormRow>
        </Stack>
      </Form>
    </Modal>
  );
}
