import { open } from "@tauri-apps/plugin-dialog";
import { Button, Field, IconButton, Mono, Stack } from "../../../../ui";
import { Plus, X } from "../../../../ui/icons";

export interface PathListProps {
  label: string;
  description?: string;
  title: string;
  paths: string[];
  onChange: (paths: string[]) => void;
}

/** An editable list of folders: add through the folder dialog, remove one by one. */
export function PathList({ label, description, title, paths, onChange }: PathListProps) {
  async function add() {
    const picked = await open({ directory: true, multiple: false, title });
    if (!picked || Array.isArray(picked) || paths.includes(picked)) return;
    onChange([...paths, picked]);
  }

  return (
    <Field label={label} description={description}>
      <Stack gap={2}>
        {paths.map((p) => (
          <Stack key={p} direction="row" gap={2} align="center">
            <Mono>{p}</Mono>
            <IconButton icon={X} label={`Remove ${p}`} size="sm" variant="ghost" onClick={() => onChange(paths.filter((x) => x !== p))} />
          </Stack>
        ))}
        <div>
          <Button size="sm" icon={Plus} onClick={add}>
            Add folder
          </Button>
        </div>
      </Stack>
    </Field>
  );
}
