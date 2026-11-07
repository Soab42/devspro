import { useMemo, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useConfirm } from "../../../app/ConfirmContext";
import type { MenuItem } from "../../../ui";
import { BookmarkPlus, Copy, Pencil, Trash2 } from "../../../ui/icons";
import { groupCommands, matchesQuery } from "../library";
import type { CommandLibrary } from "../useCommandLibrary";
import type { LibraryCommand } from "../types";
import { CommandForm, emptyDraft, type CommandDraft } from "./CommandForm";

export type ScopeFilter = "all" | "global" | "personal";

export const SCOPE_OPTIONS: { value: ScopeFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "global", label: "Global" },
  { value: "personal", label: "Personal" },
];

export const copyText = (text: string) => writeText(text);

/**
 * What the Commands page and the terminal's panel share: the search and scope
 * filter, the add/edit form, and each row's menu.
 */
export function useLibraryView(library: CommandLibrary, onError: (message: string) => void) {
  const confirmAction = useConfirm();
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<ScopeFilter>("all");
  const [form, setForm] = useState<CommandDraft | null>(null);

  const visible = useMemo(
    () => library.commands.filter((c) => (scope === "all" || c.scope === scope) && matchesQuery(c, query)),
    [library.commands, scope, query],
  );
  const sections = useMemo(() => groupCommands(visible), [visible]);

  function edit(command: LibraryCommand) {
    const own = library.personal.find((c) => c.id === command.id);
    if (own) setForm({ ...own });
  }

  async function remove(command: LibraryCommand) {
    const ok = await confirmAction(`"${command.title}" will be removed from your personal commands.`, {
      title: "Delete this command?",
      confirmLabel: "Delete",
      danger: true,
    });
    if (ok) await library.removePersonal(command.id).catch((e) => onError(String(e)));
  }

  function menuItems(command: LibraryCommand): MenuItem[] {
    const copy: MenuItem = {
      key: "copy",
      label: "Copy",
      icon: Copy,
      onClick: () => void copyText(command.command).catch((e) => onError(String(e))),
    };
    if (command.scope === "global") {
      return [
        copy,
        {
          key: "save",
          label: "Save a copy to Personal",
          icon: BookmarkPlus,
          onClick: () =>
            setForm({
              title: command.title,
              command: command.command,
              description: command.description,
              category: command.category,
              danger: command.danger,
            }),
        },
      ];
    }
    return [
      copy,
      { key: "edit", label: "Edit", icon: Pencil, onClick: () => edit(command) },
      { key: "delete", label: "Delete", icon: Trash2, danger: true, separatorBefore: true, onClick: () => void remove(command) },
    ];
  }

  const formElement = form && (
    <CommandForm
      initial={form}
      categories={library.categories}
      onSave={library.savePersonal}
      onClose={() => setForm(null)}
    />
  );

  return {
    query,
    setQuery,
    scope,
    setScope,
    sections,
    matches: visible.length,
    addNew: () => setForm({ ...emptyDraft }),
    menuItems,
    formElement,
  };
}
