/**
 * Pure helpers for the Web Server & SSL module's forms — kept out of the components
 * so the suggestion/slug rules are unit-tested rather than eyeballed.
 */

import type { DevsproContainer } from "../databases/api";

export type WizardTemplate = "laravel" | "existing" | "blank";

/** Mirrors `WizardPlan` in src-tauri/src/modules/scaffold/wizard.rs. */
export interface WizardPlan {
  name: string;
  parent_dir: string;
  template: WizardTemplate;
  domain: string;
  php_version: string | null;
  create_database: boolean;
  db_connection: string | null;
  db_name: string | null;
  write_env: boolean;
  run_migrations: boolean;
  open_in_editor: string | null;
}

export interface WizardStepReport {
  step: string;
  ok: boolean;
  message: string;
}

/** Hostname-safe slug: lowercase, runs of anything else collapsed to `-`. */
export function slugForHostname(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** `<slug>.test` for a project name, or "" when nothing usable is left. */
export function suggestDomain(name: string): string {
  const slug = slugForHostname(name);
  return slug ? `${slug}.test` : "";
}

/**
 * A database identifier the backend's `valid_schema_name` accepts: lowercase,
 * `[a-z0-9_]` only, and not starting with a digit (SQL identifiers can't).
 */
export function dbNameSlug(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^[^a-z]+|_+$/g, "");
}

/** Splits a chosen project folder into what the plan wants: its parent + basename. */
export function splitProjectFolder(path: string): { parentDir: string; name: string } {
  const trimmed = path.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (idx < 0) return { parentDir: "", name: trimmed };
  const parent = trimmed.slice(0, idx);
  // A folder at the root of a Windows drive: the parent is `C:\`, not `C:` —
  // a bare drive letter means "the current directory on C:", so `C:` joined
  // with `shop` would be `C:shop`, somewhere else entirely.
  if (/^[A-Za-z]:$/.test(parent)) return { parentDir: `${parent}\\`, name: trimmed.slice(idx + 1) };
  return { parentDir: parent || "/", name: trimmed.slice(idx + 1) };
}

/**
 * `parent` and `name` joined for display, with the separator `parent` already
 * uses — a Windows path keeps its backslashes rather than gaining a `/`.
 */
export function joinFolder(parent: string, name: string): string {
  const sep = parent.includes("\\") || /^[A-Za-z]:/.test(parent) ? "\\" : "/";
  return `${parent.replace(/[\\/]+$/, "")}${sep}${name}`;
}

/**
 * A managed database server as a Select option, or null when Laravel can't use
 * it as `DB_CONNECTION` (Redis is a cache, not a database). Container names are
 * `devspro_<service>`; the plan wants the bare service name.
 */
export function connectionOption(c: DevsproContainer): { value: string; label: string } | null {
  const service = c.name.replace(/^devspro_/, "");
  const image = c.image.split(":")[0] ?? c.image;
  if (!["mysql", "postgres", "mongo"].includes(image)) return null;
  const running = c.state === "running" ? "" : ` (${c.state})`;
  return { value: service, label: `${service} — ${c.image}${running}` };
}

export const TEMPLATE_LABELS: Record<WizardTemplate, string> = {
  laravel: "New Laravel project",
  existing: "Existing folder",
  blank: "Empty folder",
};

/** Step ids in execution order, with the labels the progress list shows. */
export const STEP_LABELS: Record<string, string> = {
  scaffold: "Project files",
  site: "Nginx site + certificate",
  hosts: "Hosts entry",
  database: "Database",
  env: ".env",
  artisan: "Artisan key:generate + migrate",
  editor: "Open in editor",
};

/** Mirrors `planned_steps` in wizard.rs, so pending rows can show before the command returns. */
export function plannedSteps(plan: WizardPlan): string[] {
  const steps = ["scaffold", "site", "hosts"];
  if (plan.create_database) steps.push("database");
  if (plan.write_env) steps.push("env");
  if (plan.run_migrations) steps.push("artisan");
  if (plan.open_in_editor?.trim()) steps.push("editor");
  return steps;
}

/** Human-readable review of the plan, one row per decision. */
export function planSummary(plan: WizardPlan): { label: string; value: string }[] {
  const rows = [
    { label: "Template", value: TEMPLATE_LABELS[plan.template] },
    { label: "Folder", value: joinFolder(plan.parent_dir, plan.name) },
    { label: "Domain", value: `https://${plan.domain}` },
    { label: "PHP", value: plan.php_version ?? "System default" },
    {
      label: "Database",
      value: plan.create_database
        ? `create ${plan.db_name ?? ""} on ${plan.db_connection ?? ""}`
        : "none",
    },
    { label: ".env", value: plan.write_env ? "write APP_URL and DB_* keys" : "leave untouched" },
    { label: "Migrations", value: plan.run_migrations ? "run" : "skip" },
  ];
  if (plan.open_in_editor?.trim()) rows.push({ label: "Editor", value: plan.open_in_editor.trim() });
  return rows;
}

export type StepState = "pending" | "running" | "ok" | "bad" | "skipped";

/**
 * What the progress row for `step` should show: results are authoritative,
 * otherwise the first unreported step is the running one and later ones wait.
 * After a failure the remaining steps never ran, so they read as skipped.
 */
export function stepState(
  step: string,
  steps: string[],
  results: WizardStepReport[] | null,
  running: boolean,
): StepState {
  const found = results?.find((r) => r.step === step);
  if (found) return found.ok ? "ok" : "bad";
  const failed = results?.some((r) => !r.ok) ?? false;
  if (failed) return "skipped";
  if (!running) return "pending";
  const firstUnreported = steps.find((s) => !results?.some((r) => r.step === s));
  return firstUnreported === step ? "running" : "pending";
}
