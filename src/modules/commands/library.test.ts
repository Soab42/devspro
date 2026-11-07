import { describe, expect, it } from "vitest";
import {
  PERSONAL_GROUP,
  UNCATEGORISED,
  fillPlaceholders,
  groupCommands,
  matchesQuery,
  parsePersonal,
  personalToLibrary,
  placeholders,
} from "./library";
import type { LibraryCommand } from "./types";

const command = (over: Partial<LibraryCommand>): LibraryCommand => ({
  id: "x",
  scope: "global",
  title: "Follow a container's logs",
  command: "docker logs -f <container>",
  description: "Streams new output.",
  tags: ["logs"],
  danger: false,
  group: "Containers & Cloud",
  category: "Docker",
  ...over,
});

describe("placeholders", () => {
  it("lists each <name> once, in order, and ignores redirection", () => {
    expect(placeholders("mysql -u <user> -p <database> < <file>")).toEqual(["user", "database", "file"]);
    expect(placeholders("scp <file> <user>@<host>:<file>")).toEqual(["file", "user", "host"]);
    expect(placeholders("ls -lah")).toEqual([]);
  });

  it("fills what was given and leaves the rest marked", () => {
    expect(fillPlaceholders("scp <file> <user>@<host>:/tmp", { file: "a.txt", user: " me " })).toBe(
      "scp a.txt me@<host>:/tmp",
    );
  });
});

describe("matchesQuery", () => {
  it("needs every word, found anywhere, in any order", () => {
    const c = command({});
    expect(matchesQuery(c, "")).toBe(true);
    expect(matchesQuery(c, "LOGS docker")).toBe(true);
    expect(matchesQuery(c, "container streams")).toBe(true);
    expect(matchesQuery(c, "logs nginx")).toBe(false);
  });
});

describe("groupCommands", () => {
  it("keeps sections and categories in arrival order", () => {
    const sections = groupCommands([
      command({ id: "1", group: "Development", category: "Git" }),
      command({ id: "2", group: "Containers & Cloud", category: "Docker" }),
      command({ id: "3", group: "Development", category: "Git" }),
      command({ id: "4", group: "Development", category: "Go" }),
    ]);
    expect(sections.map((s) => s.name)).toEqual(["Development", "Containers & Cloud"]);
    expect(sections[0].categories.map((c) => [c.key, c.commands.map((x) => x.id)])).toEqual([
      ["Development/Git", ["1", "3"]],
      ["Development/Go", ["4"]],
    ]);
  });
});

describe("personal commands", () => {
  it("survive a round trip and shrug off junk", () => {
    expect(parsePersonal(null)).toEqual([]);
    expect(parsePersonal("not json")).toEqual([]);
    expect(parsePersonal('{"a":1}')).toEqual([]);
    const parsed = parsePersonal(
      JSON.stringify([{ id: "p1", title: "Deploy", command: "./deploy.sh", category: " " }, { id: 2 }]),
    );
    expect(parsed).toEqual([
      { id: "p1", title: "Deploy", command: "./deploy.sh", description: "", category: UNCATEGORISED, danger: false, createdAt: "" },
    ]);
  });

  it("are listed under Personal, sorted by category then title", () => {
    const listed = personalToLibrary([
      { id: "b", title: "Zap", command: "z", description: "", category: "Ops", danger: false, createdAt: "" },
      { id: "a", title: "Alpha", command: "a", description: "", category: "Ops", danger: true, createdAt: "" },
      { id: "c", title: "Build", command: "b", description: "", category: "Dev", danger: false, createdAt: "" },
    ]);
    expect(listed.map((c) => c.id)).toEqual(["c", "a", "b"]);
    expect(listed.every((c) => c.group === PERSONAL_GROUP && c.scope === "personal")).toBe(true);
  });
});
