import type { CatalogFile, LibraryCommand, PersonalCommand } from "./types";

/** Sections in the order they are listed; any other catalogue file follows, by name. */
const GROUP_ORDER = ["Development", "Containers & Cloud", "Server Management", "System & Network", "Databases"];

export const PERSONAL_GROUP = "Personal";
export const UNCATEGORISED = "Uncategorised";

/**
 * The global catalogue. Bundled for now, one chunk per file loaded on first
 * use; this is the one place to point at a server once there is one.
 */
export async function loadGlobalCommands(): Promise<LibraryCommand[]> {
  const loaders = import.meta.glob<CatalogFile>("./catalog/*.json", { import: "default" });
  const files = await Promise.all(Object.values(loaders).map((load) => load()));
  const rank = (group: string) => {
    const at = GROUP_ORDER.indexOf(group);
    return at < 0 ? GROUP_ORDER.length : at;
  };
  files.sort((a, b) => rank(a.group) - rank(b.group) || a.group.localeCompare(b.group));
  return files.flatMap((file) =>
    file.categories.flatMap((category) =>
      category.commands.map((c, i) => ({
        id: `global:${category.id}:${i}`,
        scope: "global" as const,
        title: c.title,
        command: c.command,
        description: c.description,
        tags: c.tags ?? [],
        danger: c.danger ?? false,
        group: file.group,
        category: category.name,
      })),
    ),
  );
}

/* ---- personal commands, kept per profile in the settings table ---- */

export const personalKey = (profileId: string) => `commands.personal.${profileId}`;

export function parsePersonal(raw: string | null | undefined): PersonalCommand[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((item): PersonalCommand[] => {
      if (typeof item !== "object" || item === null) return [];
      const c = item as Record<string, unknown>;
      if (typeof c.id !== "string" || typeof c.title !== "string" || typeof c.command !== "string") return [];
      return [
        {
          id: c.id,
          title: c.title,
          command: c.command,
          description: typeof c.description === "string" ? c.description : "",
          category: typeof c.category === "string" && c.category.trim() ? c.category.trim() : UNCATEGORISED,
          danger: c.danger === true,
          createdAt: typeof c.createdAt === "string" ? c.createdAt : "",
        },
      ];
    });
  } catch {
    return [];
  }
}

export function personalToLibrary(commands: PersonalCommand[]): LibraryCommand[] {
  return [...commands]
    .sort((a, b) => a.category.localeCompare(b.category) || a.title.localeCompare(b.title))
    .map((c) => ({
      id: c.id,
      scope: "personal" as const,
      title: c.title,
      command: c.command,
      description: c.description,
      tags: [],
      danger: c.danger,
      group: PERSONAL_GROUP,
      category: c.category,
    }));
}

/* ---- search and grouping ---- */

/** Every word of the query appears somewhere in the command's text, in any order. */
export function matchesQuery(command: LibraryCommand, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const haystack = [command.title, command.command, command.description, command.category, ...command.tags]
    .join(" ")
    .toLowerCase();
  return words.every((w) => haystack.includes(w));
}

export interface CategoryGroup {
  /** `group/category`, stable across searches, for remembering what is folded. */
  key: string;
  name: string;
  commands: LibraryCommand[];
}

export interface Section {
  name: string;
  categories: CategoryGroup[];
}

/** Sections and categories in the order their first command arrives. */
export function groupCommands(commands: LibraryCommand[]): Section[] {
  const sections: Section[] = [];
  for (const command of commands) {
    let section = sections.find((s) => s.name === command.group);
    if (!section) {
      section = { name: command.group, categories: [] };
      sections.push(section);
    }
    let category = section.categories.find((c) => c.name === command.category);
    if (!category) {
      category = { key: `${command.group}/${command.category}`, name: command.category, commands: [] };
      section.categories.push(category);
    }
    category.commands.push(command);
  }
  return sections;
}

/* ---- placeholders: `<name>` in a command, asked for before it is typed ---- */

const PLACEHOLDER = /<([a-z][a-z0-9_-]*)>/g;

/** The placeholder names in a command, each once, in the order they appear. */
export function placeholders(command: string): string[] {
  return [...new Set(Array.from(command.matchAll(PLACEHOLDER), (m) => m[1]))];
}

/** The command with each `<name>` replaced; one left empty stays as it was. */
export function fillPlaceholders(command: string, values: Record<string, string>): string {
  return command.replace(PLACEHOLDER, (whole, name: string) => values[name]?.trim() || whole);
}
