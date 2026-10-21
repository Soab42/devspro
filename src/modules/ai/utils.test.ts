import { describe, expect, it } from "vitest";
import type { AgentStatus } from "./api";
import { groupAgents, installCommand, launchCommand, rememberProject, sessionTitle, splitArgs } from "./utils";

const agent = (over: Partial<AgentStatus> = {}): AgentStatus => ({
  id: "claude-code",
  name: "Claude Code",
  vendor: "Anthropic",
  description: "",
  command: "claude",
  args: [],
  install: "npm install -g @anthropic-ai/claude-code",
  homepage: null,
  installed: true,
  path: "/usr/bin/claude",
  custom: false,
  launch: ["claude"],
  install_shell: ["sh", "-c"],
  ...over,
});

describe("splitArgs", () => {
  it("splits on whitespace and honours quotes", () => {
    expect(splitArgs("")).toEqual([]);
    expect(splitArgs("   ")).toEqual([]);
    expect(splitArgs("--resume -p  x")).toEqual(["--resume", "-p", "x"]);
    expect(splitArgs('--model "gpt 5" --name \'my agent\'')).toEqual(["--model", "gpt 5", "--name", "my agent"]);
    expect(splitArgs('--empty ""')).toEqual(["--empty", ""]);
  });

  it("treats a backslash as an escape outside single quotes", () => {
    expect(splitArgs("a\\ b c")).toEqual(["a b", "c"]);
    expect(splitArgs('"a\\"b"')).toEqual(['a"b']);
    expect(splitArgs("'a\\b'")).toEqual(["a\\b"]);
  });

  it("keeps an unterminated quote's contents rather than dropping them", () => {
    expect(splitArgs('--msg "hello world')).toEqual(["--msg", "hello world"]);
  });
});

describe("commands", () => {
  it("runs the agent in the project with fixed args before extra ones", () => {
    expect(launchCommand(agent({ args: ["--fixed"] }), "/home/me/app", "--resume")).toEqual({
      exec: { program: "claude", args: ["--fixed", "--resume"], cwd: "/home/me/app" },
    });
  });

  it("runs a Windows .cmd shim through the launch prefix the backend resolved", () => {
    const shim = "C:\\Users\\me\\AppData\\Roaming\\npm\\claude.cmd";
    expect(launchCommand(agent({ launch: ["cmd.exe", "/d", "/c", "call", shim] }), "C:\\code\\app", "--resume")).toEqual({
      exec: { program: "cmd.exe", args: ["/d", "/c", "call", shim, "--resume"], cwd: "C:\\code\\app" },
    });
  });

  it("runs an installer through sh", () => {
    expect(installCommand(["sh", "-c"], "npm i -g x")).toEqual({
      exec: { program: "sh", args: ["-c", "npm i -g x"], cwd: null },
    });
  });

  it("runs an installer through PowerShell on Windows", () => {
    const ps = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"];
    expect(installCommand(ps, "npm i -g x")).toEqual({
      exec: { program: "powershell.exe", args: [...ps.slice(1), "npm i -g x"], cwd: null },
    });
  });
});

describe("titles and projects", () => {
  it("names a session after the agent and the project folder", () => {
    expect(sessionTitle("Codex CLI", "/home/me/devspro")).toBe("Codex CLI · devspro");
    expect(sessionTitle("Codex CLI", null)).toBe("Codex CLI · ~");
    expect(sessionTitle("Codex CLI", "C:\\Users\\me\\devspro\\")).toBe("Codex CLI · devspro");
  });

  it("keeps recent projects unique, newest first and bounded", () => {
    let recent: string[] = [];
    for (let i = 0; i < 10; i += 1) recent = rememberProject(recent, `/p/${i}`);
    expect(recent).toHaveLength(8);
    expect(recent[0]).toBe("/p/9");
    expect(rememberProject(recent, "/p/5")[0]).toBe("/p/5");
    expect(rememberProject(recent, "/p/5").filter((p) => p === "/p/5")).toHaveLength(1);
  });

  it("groups installed agents ahead of missing ones", () => {
    const a = agent({ id: "a", installed: false });
    const b = agent({ id: "b" });
    const c = agent({ id: "c", installed: false });
    const groups = groupAgents([a, b, c]);
    expect(groups.installed.map((x) => x.id)).toEqual(["b"]);
    expect(groups.missing.map((x) => x.id)).toEqual(["a", "c"]);
  });
});
