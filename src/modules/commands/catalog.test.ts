import { describe, expect, it } from "vitest";
import type { CatalogFile } from "./types";

// Every catalogue file, checked the same way. Run one with
// `npx vitest run src/modules/commands/catalog.test.ts -t <file>.json`.
const files = import.meta.glob<CatalogFile>("./catalog/*.json", { eager: true, import: "default" });
const entries = Object.entries(files).map(([path, file]) => [path.replace("./catalog/", ""), file] as const);

const KEBAB = /^[a-z0-9]+(-[a-z0-9]+)*$/;
/** `<name>`: lowercase, digits, `-` or `_`, no spaces. */
const PLACEHOLDER = /<[a-z][a-z0-9_-]*>/g;
/** Anything in angle brackets with no space inside, which must then be a placeholder. */
const BRACKETED = /<[^<>\s]+>/g;
/** Commands that destroy data or lock you out must be marked `danger`. */
const MUST_BE_DANGER = [
  // `rm` as the command itself, not `git rm -r --cached`.
  /(^|[;&|]\s*|\bsudo\s+|\bxargs\s+)rm\s+-[a-zA-Z]*[rRf]/,
  /\bDROP\s+(DATABASE|TABLE|USER|SCHEMA)\b/i,
  /\bTRUNCATE\b/i,
  /\bFLUSHALL\b|\bFLUSHDB\b/i,
  /\bprune\b.*\s-a\b|\bprune\b.*--all\b/,
  /\breset\s+--hard\b/,
  /\bpush\b.*(--force|\s-f\b)/,
  /\bmkfs\b/,
  /\bdd\s+if=/,
  /\biptables\s+-F\b/,
  /\bufw\s+(reset|disable)\b/,
  /\buserdel\b/,
  /\bkubectl\s+delete\b/,
  /\bterraform\s+destroy\b/,
  /\bdropdb\b/,
  /\bdocker\s+(rm|rmi|volume\s+rm)\b.*\s-f\b/,
  /\bchmod\s+-R\s+777\b/,
  /\bshred\b/,
  // Rebooting itself, not `last reboot` or reading /var/run/reboot-required.
  /(^|[;&|]\s*|\bsudo\s+|\bsystemctl\s+)(reboot|shutdown|poweroff|halt)\b/,
];

describe.each(entries)("%s", (_name, file) => {
  it("is a section with categories", () => {
    expect(typeof file.group).toBe("string");
    expect(file.group.trim()).not.toBe("");
    expect(Array.isArray(file.categories)).toBe(true);
    expect(file.categories.length).toBeGreaterThan(0);
  });

  it("has well-formed categories", () => {
    for (const category of file.categories) {
      expect(category.id, category.id).toMatch(KEBAB);
      expect(category.name.trim(), category.id).not.toBe("");
      expect(category.commands.length, category.id).toBeGreaterThanOrEqual(5);
    }
  });

  it("has well-formed commands", () => {
    for (const category of file.categories) {
      const titles = new Set<string>();
      const commands = new Set<string>();
      for (const c of category.commands) {
        const where = `${category.id}: ${c.title}`;
        expect(c.title.trim(), where).not.toBe("");
        expect(c.title.length, where).toBeLessThanOrEqual(70);
        expect(c.command.trim(), where).not.toBe("");
        expect(c.command, where).not.toMatch(/[\r\n]/);
        expect(c.command, where).toBe(c.command.trim());
        expect(c.description.trim(), where).not.toBe("");
        expect(c.description.length, where).toBeLessThanOrEqual(200);
        if (c.tags !== undefined) {
          expect(Array.isArray(c.tags), where).toBe(true);
          for (const tag of c.tags) expect(tag, where).toMatch(/^[a-z0-9][a-z0-9 .+#-]*$/);
        }
        if (c.danger !== undefined) expect(typeof c.danger, where).toBe("boolean");
        expect(titles.has(c.title), `duplicate title ${where}`).toBe(false);
        expect(commands.has(c.command), `duplicate command ${where}`).toBe(false);
        titles.add(c.title);
        commands.add(c.command);
      }
    }
  });

  it("writes placeholders as <lowercase-name>", () => {
    for (const category of file.categories) {
      for (const c of category.commands) {
        const bracketed = c.command.match(BRACKETED) ?? [];
        const placeholders = c.command.match(PLACEHOLDER) ?? [];
        expect(bracketed, `${category.id}: ${c.command}`).toEqual(placeholders);
      }
    }
  });

  it("marks destructive commands as danger", () => {
    for (const category of file.categories) {
      for (const c of category.commands) {
        if (MUST_BE_DANGER.some((re) => re.test(c.command))) {
          expect(c.danger, `${category.id}: ${c.command} needs "danger": true`).toBe(true);
        }
      }
    }
  });
});

describe("the whole catalogue", () => {
  const categories = entries.flatMap(([, file]) => file.categories);

  it("never reuses a category id", () => {
    const ids = categories.map((c) => c.id);
    expect(ids.filter((id, i) => ids.indexOf(id) !== i)).toEqual([]);
  });

  it("holds at least 500 commands", () => {
    const total = categories.reduce((n, c) => n + c.commands.length, 0);
    expect(total).toBeGreaterThanOrEqual(500);
  });
});
