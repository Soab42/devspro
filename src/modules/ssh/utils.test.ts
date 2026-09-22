import { describe, expect, it } from "vitest";
import type { SftpEntry, SshHost } from "./api";
import {
  baseName,
  formatModified,
  formatSize,
  hostLabel,
  isPasswordRequired,
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
    { id: "1", name: "Prod", host: "p", port: 22, username: "u", auth: "agent", key_path: null, created_at: "" },
  ];

  it("names local shells and saved hosts, numbering repeats", () => {
    expect(terminalTitle("local", hosts, 1)).toBe("Local");
    expect(terminalTitle("local", hosts, 3)).toBe("Local 3");
    expect(terminalTitle({ ssh: "1" }, hosts, 1)).toBe("Prod");
    expect(terminalTitle({ ssh: "1" }, hosts, 2)).toBe("Prod (2)");
    expect(terminalTitle({ ssh: "missing" }, hosts, 1)).toBe("ssh");
  });
});
