import type { BackupSection, BackupSectionCount } from "./api";

/** Human labels for the bundle sections, in the order the import dialog lists them. */
export const BACKUP_SECTION_LABELS: Record<BackupSection, string> = {
  sites: "Sites",
  workers: "Workers",
  settings: "Preferences",
  ca: "Root CA",
};

export const BACKUP_SECTION_ORDER: BackupSection[] = [
  "sites",
  "workers",
  "settings",
  "ca",
];

/** `devspro-backup-2026-09-22.json` — the date so several exports sort by age. */
export function defaultBackupFilename(now: Date): string {
  return `devspro-backup-${now.toISOString().slice(0, 10)}.json`;
}

/**
 * Sections worth offering: a section with nothing in it is left unchecked so a
 * Replace import of an empty section can't wipe the local rows by accident.
 */
export function defaultSelection(
  sections: BackupSectionCount[],
): BackupSection[] {
  return sections.filter((s) => s.count > 0).map((s) => s.section);
}

/** "2 sites, 1 worker, 3 preferences, root CA" for the summary callouts. */
export function describeCounts(sections: BackupSectionCount[]): string {
  const parts = sections
    .filter((s) => s.count > 0)
    .map((s) => {
      if (s.section === "ca") return "root CA";
      const label = BACKUP_SECTION_LABELS[s.section].toLowerCase();
      const singular = label.endsWith("s") ? label.slice(0, -1) : label;
      return `${s.count} ${s.count === 1 ? singular : label}`;
    });
  return parts.length ? parts.join(", ") : "nothing";
}
