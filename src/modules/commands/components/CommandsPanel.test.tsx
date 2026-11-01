import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import CommandsPanel from "./CommandsPanel";

const settings = new Map<string, string>();
const invoke = vi.fn(async (cmd: string, args: { key: string; value?: string }) => {
  if (cmd === "settings_get") return settings.get(args.key) ?? null;
  if (cmd === "settings_set") settings.set(args.key, args.value ?? "");
  return undefined;
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: (cmd: string, args: { key: string }) => invoke(cmd, args) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => {}) }));
vi.mock("../../../auth/AuthContext", () => ({ useAuth: () => ({ profile: { id: "p1" } }) }));

beforeEach(() => {
  cleanup();
  settings.clear();
  invoke.mockClear();
  localStorage.clear();
});

function renderPanel() {
  const write = vi.fn();
  render(
    <ConfirmProvider>
      <CommandsPanel write={write} onClose={() => {}} onError={() => {}} />
    </ConfirmProvider>,
  );
  return write;
}

const search = (text: string) => fireEvent.change(screen.getByPlaceholderText("Search commands…"), { target: { value: text } });
const row = (title: string) => screen.getByText(title).closest("li") as HTMLElement;

describe("the terminal's command panel", () => {
  it("lists the library by section, categories folded until opened", async () => {
    renderPanel();
    expect(await screen.findByRole("region", { name: "Containers & Cloud" })).toBeDefined();
    const docker = screen.getByRole("button", { name: /^Docker\s*\d+$/ });
    expect(docker.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("List running containers")).toBeNull();
    fireEvent.click(docker);
    expect(screen.getByText("List running containers")).toBeDefined();
  });

  it("types a command in without running it", async () => {
    const write = renderPanel();
    await screen.findByRole("region", { name: "Containers & Cloud" });
    search("list running containers");
    fireEvent.click(screen.getByText("List running containers"));
    expect(write).toHaveBeenCalledWith("docker ps");
  });

  it("asks for placeholders, then types the filled-in command", async () => {
    const write = renderPanel();
    await screen.findByRole("region", { name: "Server Management" });
    search("show a service's status");
    fireEvent.click(screen.getByText("Show a service's status"));
    const dialog = await screen.findByRole("dialog", { name: "Show a service's status" });
    fireEvent.change(within(dialog).getByLabelText("service"), { target: { value: "nginx" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Type it in" }));
    expect(write).toHaveBeenCalledWith("systemctl status nginx");
  });

  it("asks before running a destructive command, and sends Enter only once confirmed", async () => {
    const write = renderPanel();
    await screen.findByRole("region", { name: "Containers & Cloud" });
    search("remove all stopped containers");
    // Shown on hover, which jsdom can't do; hidden, it has no accessible name, so find it by label.
    fireEvent.click(within(row("Remove all stopped containers")).getByLabelText("Run"));
    const confirm = await screen.findByRole("dialog", { name: 'Run "Remove all stopped containers"?' });
    expect(write).not.toHaveBeenCalled();
    fireEvent.click(within(confirm).getByRole("button", { name: "Run it" }));
    await waitFor(() => expect(write).toHaveBeenCalledWith("docker container prune\r"));
  });

  it("adds a personal command, saved for this profile", async () => {
    renderPanel();
    await screen.findByRole("region", { name: "Containers & Cloud" });
    fireEvent.click(screen.getByRole("button", { name: "Add a command" }));
    const form = await screen.findByRole("dialog", { name: "New command" });
    fireEvent.change(within(form).getByLabelText(/Title/), { target: { value: "Deploy the API" } });
    fireEvent.change(within(form).getByLabelText(/Command/), { target: { value: "./deploy.sh api" } });
    fireEvent.change(within(form).getByLabelText(/Category/), { target: { value: "Deploy" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add command" }));

    await waitFor(() => expect(settings.has("commands.personal.p1")).toBe(true));
    expect(JSON.parse(settings.get("commands.personal.p1") ?? "[]")).toMatchObject([
      { title: "Deploy the API", command: "./deploy.sh api", category: "Deploy", danger: false },
    ]);
    fireEvent.click(screen.getByRole("radio", { name: "Personal" }));
    expect(await screen.findByRole("region", { name: "Personal" })).toBeDefined();
  });
});
