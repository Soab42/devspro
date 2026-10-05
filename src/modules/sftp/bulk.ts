import { errorMessage } from "../ssh/utils";

/** "1 item", "3 items". */
export function countLabel(n: number): string {
  return `${n} ${n === 1 ? "item" : "items"}`;
}

export interface BulkFailure {
  name: string;
  error: string;
}

/**
 * Runs `action` on each entry in turn, never stopping at a failure: one file
 * the server refuses shouldn't strand the rest of the selection. One at a
 * time because the Rust side holds the session for each call anyway.
 */
export async function settleEach<T extends { name: string }>(
  entries: T[],
  action: (entry: T) => Promise<unknown>,
): Promise<BulkFailure[]> {
  const failures: BulkFailure[] = [];
  for (const entry of entries) {
    try {
      await action(entry);
    } catch (e) {
      failures.push({ name: entry.name, error: errorMessage(e) });
    }
  }
  return failures;
}

/** "Couldn't delete 2 of 5 items: a (why); b (why)". */
export function failureSummary(verb: string, failures: BulkFailure[], total: number): string {
  const which = failures.map((f) => `${f.name} (${f.error})`).join("; ");
  return `Couldn't ${verb} ${failures.length} of ${countLabel(total)}: ${which}`;
}

/** The archive's default name: the one entry it holds, else the folder they came from. */
export function zipName(entries: { name: string }[], dir: string | null, fallback: string): string {
  const base =
    entries.length === 1 ? entries[0].name : (dir ?? "").split("/").filter(Boolean).pop() || fallback;
  return `${base}.zip`;
}
