/**
 * The command library: a global catalogue every install shares, and each
 * profile's personal commands. The catalogue ships in the app as JSON files
 * under ./catalog, in the shape a server would serve it, so it can move to
 * one later without the UI changing.
 */

/** One file under ./catalog: a section of the library and its categories. */
export interface CatalogFile {
  /** The section heading the categories sit under, e.g. "Development". */
  group: string;
  categories: CatalogCategory[];
}

export interface CatalogCategory {
  /** kebab-case, unique across every catalogue file. */
  id: string;
  name: string;
  commands: CatalogCommand[];
}

export interface CatalogCommand {
  title: string;
  /**
   * One line. `<name>` marks a placeholder (lowercase, no spaces) that is
   * asked for before the command is typed in.
   */
  command: string;
  description: string;
  tags?: string[];
  /** Destructive or hard to undo: running it asks first. */
  danger?: boolean;
}

export type CommandScope = "global" | "personal";

/** A command as the UI works with it, from either scope. */
export interface LibraryCommand {
  id: string;
  scope: CommandScope;
  title: string;
  command: string;
  description: string;
  tags: string[];
  danger: boolean;
  /** Section heading: the catalogue file's group, or "Personal". */
  group: string;
  category: string;
}

/** What a profile saves: its own commands, grouped by a free-text category. */
export interface PersonalCommand {
  id: string;
  title: string;
  command: string;
  description: string;
  category: string;
  danger: boolean;
  createdAt: string;
}
