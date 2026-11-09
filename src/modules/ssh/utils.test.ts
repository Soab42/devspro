import { describe, expect, it } from "vitest";
import type { SftpEntry, SshHost } from "./api";
import {
  baseName,
  cdCommand,
  followHookCommand,
  shellQuote,
  formatModified,
  formatSize,
  hostLabel,
  isAuthFailure,
  isPasswordRequired,
  keysIgnoredByAgentMode,
  samePath,
  untriedKeys,
  joinPath,
  parentPath,
  sortEntries,
  splitBreadcrumb,
  terminalTitle,
  unknownHostFingerprint,
  validateHostForm,
} from "./utils";

const entry = (over: Partial<SftpEntry>): SftpEntry => ({
  name: "x",
  path: "/x",
  is_dir: false,
  size: 0,
  modified: 0,
  permissions: "644",
  ...over,
});

describe("splitBreadcrumb", () => {
  it("always starts at root and accumulates absolute paths", () => {
    expect(splitBreadcrumb("/var/log/nginx")).toEqual([
      { label: "/", path: "/" },
      { label: "var", path: "/var" },
      { label: "log", path: "/var/log" },
      { label: "nginx", path: "/var/log/nginx" },
    ]);
  });

  it("collapses the root and trailing slashes", () => {
    expect(splitBreadcrumb("/")).toEqual([{ label: "/", path: "/" }]);
    expect(splitBreadcrumb("/home/me/")).toHaveLength(3);
  });
});

describe("paths", () => {
  it("parentPath stops at the root", () => {
    expect(parentPath("/var/log")).toBe("/var");
    expect(parentPath("/var")).toBe("/");
    expect(parentPath("/")).toBe("/");
    expect(parentPath("/var/log/")).toBe("/var");
  });

  it("joinPath never doubles the separator", () => {
    expect(joinPath("/", "etc")).toBe("/etc");
    expect(joinPath("/home/me", "x")).toBe("/home/me/x");
    expect(joinPath("/home/me/", "x")).toBe("/home/me/x");
  });

  it("baseName returns the last segment", () => {
    expect(baseName("/a/b/c.txt")).toBe("c.txt");
    expect(baseName("/")).toBe("");
  });
});

describe("formatSize", () => {
  it("picks a unit and trims precision for large values", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(150 * 1024)).toBe("150 KB");
    expect(formatSize(3 * 1024 * 1024)).toBe("3.0 MB");
    expect(formatSize(-1)).toBe("—");
  });
});

describe("formatModified", () => {
  it("renders a compact local timestamp and a dash for unknown", () => {
    expect(formatModified(0)).toBe("—");
    expect(formatModified(1_700_000_000)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });
});

describe("sortEntries", () => {
  const rows = [
    entry({ name: "b.txt", size: 20, modified: 2 }),
    entry({ name: "zdir", is_dir: true, modified: 9 }),
    entry({ name: "a.txt", size: 30, modified: 1 }),
    entry({ name: "adir", is_dir: true, modified: 5 }),
  ];

  it("keeps directories first in both directions", () => {
    const asc = sortEntries(rows, "name", "asc").map((e) => e.name);
    expect(asc).toEqual(["adir", "zdir", "a.txt", "b.txt"]);
    const desc = sortEntries(rows, "name", "desc").map((e) => e.name);
    expect(desc).toEqual(["zdir", "adir", "b.txt", "a.txt"]);
  });

  it("sorts by size and modified within each group", () => {
    expect(sortEntries(rows, "size", "asc").map((e) => e.name)).toEqual(["adir", "zdir", "b.txt", "a.txt"]);
    expect(sortEntries(rows, "modified", "desc").map((e) => e.name)).toEqual(["zdir", "adir", "b.txt", "a.txt"]);
  });

  it("does not mutate its input", () => {
    const copy = [...rows];
    sortEntries(rows, "name", "desc");
    expect(rows).toEqual(copy);
  });
});

describe("hostLabel", () => {
  it("omits the default port", () => {
    expect(hostLabel({ username: "me", host: "h", port: 22 })).toBe("me@h");
    expect(hostLabel({ username: "me", host: "h", port: 2222 })).toBe("me@h:2222");
  });
});

describe("connect error contracts", () => {
  it("extracts the fingerprint from an unknown-host error", () => {
    expect(unknownHostFingerprint("UNKNOWN_HOST:SHA256:abc")).toBe("SHA256:abc");
    expect(unknownHostFingerprint("authentication failed")).toBeNull();
  });

  it("recognises the password prompt sentinel", () => {
    expect(isPasswordRequired("PASSWORD_REQUIRED")).toBe(true);
    expect(isPasswordRequired("password rejected")).toBe(false);
  });
});

describe("validateHostForm", () => {
  const ok = { name: "n", host: "h", port: "22", username: "u", auth: "agent", keyPath: "" };

  it("accepts a complete form", () => {
    expect(validateHostForm(ok)).toBeNull();
  });

  it("names the first missing field", () => {
    expect(validateHostForm({ ...ok, name: " " })).toMatch(/name/i);
    expect(validateHostForm({ ...ok, host: "" })).toMatch(/host/i);
    expect(validateHostForm({ ...ok, host: "a b" })).toMatch(/spaces/i);
    expect(validateHostForm({ ...ok, port: "0" })).toMatch(/port/i);
    expect(validateHostForm({ ...ok, port: "abc" })).toMatch(/port/i);
    expect(validateHostForm({ ...ok, username: "" })).toMatch(/username/i);
    expect(validateHostForm({ ...ok, auth: "key" })).toMatch(/key/i);
    expect(validateHostForm({ ...ok, auth: "key", keyPath: "/k" })).toBeNull();
  });
});

describe("terminalTitle", () => {
  const hosts: SshHost[] = [
    { id: "1", name: "Prod", host: "p", port: 22, username: "u", auth: "agent", key_path: null, created_at: "", has_saved_password: false },
  ];

  it("names local shells and saved hosts, numbering repeats", () => {
    expect(terminalTitle("local", hosts, 1)).toBe("Local");
    expect(terminalTitle("local", hosts, 3)).toBe("Local 3");
    expect(terminalTitle({ ssh: "1" }, hosts, 1)).toBe("Prod");
    expect(terminalTitle({ ssh: "1" }, hosts, 2)).toBe("Prod (2)");
    expect(terminalTitle({ ssh: "missing" }, hosts, 1)).toBe("ssh");
  });
});

describe("shell helpers", () => {
  it("quotes paths for a POSIX shell", () => {
    expect(shellQuote("/tmp/a b")).toBe("'/tmp/a b'");
    expect(shellQuote("it's")).toBe("'it'\\''s'");
    expect(cdCommand("/var/www")).toBe(" cd '/var/www'\n");
  });

  it("installs a prompt hook that expands at each prompt, not at install time", () => {
    const hook = followHookCommand();
    expect(hook.startsWith(" ")).toBe(true);
    expect(hook.endsWith("\n")).toBe(true);
    // The bash branch must single-quote the assignment, or $PWD is baked in once.
    expect(hook).toMatch(/PROMPT_COMMAND='printf "%b" "\\033\]7;file:\/\/\$\(hostname 2>\/dev\/null\)\$PWD\\007"'/);
    expect(hook).toContain("precmd()");
  });
});

describe("auth failure hints", () => {
  const failure =
    "authentication failed for ploi@52.62.3.173: ssh-agent has no keys loaded; not tried (not a default name, not in ~/.ssh/config): dealercore-server, work";

  it("recognises an auth failure and reads the keys nothing offered", () => {
    expect(isAuthFailure(failure)).toBe(true);
    expect(isAuthFailure("could not connect to x:22")).toBe(false);
    expect(untriedKeys(failure)).toEqual(["dealercore-server", "work"]);
    expect(untriedKeys("authentication failed for a@b: password rejected")).toEqual([]);
  });

  it("lists only keys agent mode leaves out", () => {
    const key = (name: string, path: string, has_private = true) => ({ name, path, has_private });
    const keys = [
      key("id_ed25519", String.raw`C:\Users\me\.ssh\id_ed25519`),
      key("dealercore-server", String.raw`C:\Users\me\.ssh\dealercore-server`),
      key("configured", String.raw`C:\Users\me\.ssh\configured`),
      key("public-only", String.raw`C:\Users\me\.ssh\public-only`, false),
    ];
    const ignored = keysIgnoredByAgentMode(keys, ["c:/users/ME/.ssh/configured"]);
    expect(ignored.map((k) => k.name)).toEqual(["dealercore-server"]);
  });

  it("compares key paths across slash styles", () => {
    expect(samePath(String.raw`C:\Users\Me\.ssh\k`, "c:/users/me/.ssh/k")).toBe(true);
    expect(samePath("/home/me/.ssh/K", "/home/me/.ssh/k")).toBe(false);
  });
});
