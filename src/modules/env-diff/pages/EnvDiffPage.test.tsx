import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import EnvDiffPage from "./EnvDiffPage";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
// CodeMirror needs layout jsdom doesn't have; a textarea keeps the same
// value/onChange contract, which is all these tests exercise.
vi.mock("../../../ui/CodeEditor", () => ({
  CodeEditor: ({
    id,
    value,
    onChange,
  }: {
    id?: string;
    value: string;
    onChange: (v: string) => void;
  }) => <textarea id={id} value={value} onChange={(e) => onChange(e.target.value)} />,
}));

function renderPage() {
  return render(
    <ConfirmProvider>
      <EnvDiffPage />
    </ConfirmProvider>,
  );
}

const contents = () => screen.getAllByLabelText("Contents") as HTMLTextAreaElement[];
const paste = (box: HTMLTextAreaElement, text: string) =>
  fireEvent.change(box, { target: { value: text } });

const table = () => screen.getByRole("table", { name: /variables in the left and right/i });
const rowFor = (key: string) => within(table()).getByText(key).closest("tr") as HTMLElement;

beforeEach(() => {
  cleanup();
  invoke.mockReset();
  // The real invoke always returns a promise; code may chain on it.
  invoke.mockResolvedValue(undefined);
});

describe("EnvDiffPage", () => {
  it("starts empty and says what to do", () => {
    renderPage();
    expect(screen.getByText("Nothing to compare yet")).toBeDefined();
  });

  it("shows missing, new and changed variables, and hides the ones that match", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "SAME=1\nCHANGED=a\nONLY_LEFT=x\n");
    paste(right, "ONLY_RIGHT=y\nCHANGED=b\nSAME=1\n");

    expect(within(rowFor("ONLY_LEFT")).getByText("Only in left")).toBeDefined();
    expect(within(rowFor("ONLY_RIGHT")).getByText("Only in right")).toBeDefined();
    expect(within(rowFor("CHANGED")).getByText("Different value")).toBeDefined();
    expect(within(table()).queryByText("SAME")).toBeNull();
  });

  it("reports identical files rather than an empty table", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "A=1\nB=2\n");
    paste(right, "B=2\n\n# moved\nA=1\n");
    expect(screen.getByText("No differences")).toBeDefined();
  });

  it("keeps values masked until asked", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "TOKEN=hunter2\n");
    paste(right, "TOKEN=letmein\n");

    expect(within(rowFor("TOKEN")).queryByText("hunter2")).toBeNull();
    fireEvent.click(screen.getByRole("switch", { name: "Show values" }));
    expect(within(rowFor("TOKEN")).getByText("hunter2")).toBeDefined();
    expect(within(rowFor("TOKEN")).getByText("letmein")).toBeDefined();
  });

  it("ignores value differences in keys-only mode", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "A=1\nB=2\n");
    paste(right, "A=9\n");

    expect(within(table()).getByText("A")).toBeDefined();
    fireEvent.click(screen.getByRole("switch", { name: "Keys only" }));
    expect(within(table()).queryByText("A")).toBeNull();
    expect(within(table()).getByText("B")).toBeDefined();
  });

  it("copies a missing variable across without disturbing the file's layout", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "A=1\nNEW=fresh\n");
    paste(right, "# mine\nA=1\n");

    fireEvent.click(within(rowFor("NEW")).getByRole("button", { name: "Add to right" }));
    expect(contents()[1].value).toBe("# mine\nA=1\nNEW=fresh\n");
    expect(screen.getByText("No differences")).toBeDefined();
  });

  it("copies a different value across in place", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "A=old\nB=2\n");
    paste(right, "B=2\nA=new\n");

    fireEvent.click(within(rowFor("A")).getByRole("button", { name: "Use right value on left" }));
    expect(contents()[0].value).toBe("A=new\nB=2\n");
  });

  it("adds every missing variable in one go", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "A=1\nB=2\nC=3\n");
    paste(right, "A=1\n");

    fireEvent.click(screen.getByRole("button", { name: "Add 2 missing to right" }));
    expect(contents()[1].value).toBe("A=1\nB=2\nC=3\n");
    expect(screen.getByRole("button", { name: "Add 0 missing to right" }).hasAttribute("disabled")).toBe(
      true,
    );
  });

  it("flags a repeated variable inside one file", () => {
    renderPage();
    paste(contents()[0], "A=1\nA=2\n");
    expect(screen.getByText(/already set on line 1/)).toBeDefined();
  });

  it("marks a likely rename, and finds both halves when searching either name", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "DB_HOST=db.internal\nA=1\n");
    paste(right, "A=1\nDATABASE_HOST=db.internal\n");

    expect(within(rowFor("DB_HOST")).getByText("Renamed to DATABASE_HOST?")).toBeDefined();
    expect(within(rowFor("DATABASE_HOST")).getByText("Renamed from DB_HOST?")).toBeDefined();

    fireEvent.change(screen.getByLabelText("Filter by variable name"), {
      target: { value: "database" },
    });
    expect(within(table()).getByText("DB_HOST")).toBeDefined();
  });

  it("flags a variable commented out on one side, and switches it back on", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "API_URL=x\nOLD=1\n");
    paste(right, "# API_URL=x\nOLD=1\n# LEGACY=y\n");

    expect(within(rowFor("API_URL")).getByText("Commented out in right")).toBeDefined();
    expect(within(rowFor("LEGACY")).getByText("Commented out")).toBeDefined();
    fireEvent.click(within(rowFor("API_URL")).getByRole("button", { name: "Uncomment on right" }));
    expect(contents()[1].value).toBe("API_URL=x\nOLD=1\n# LEGACY=y\n");
  });

  it("arranges one file like the other, so the same variables share line numbers", () => {
    renderPage();
    const [left, right] = contents();
    paste(left, "# db\nDB=1\nAPP=2\n");
    paste(right, "APP=9\nEXTRA=x\nDB=8\n");

    fireEvent.click(screen.getByRole("button", { name: "Arrange right like left" }));
    expect(contents()[1].value).toBe("# db\nDB=8\nAPP=9\n\nEXTRA=x\n");
    expect(contents()[0].value).toBe("# db\nDB=1\nAPP=2\n");
    expect(screen.getByText(/Its 1 own variable is at the end/)).toBeDefined();
  });

  it("flags two variables holding the same value inside one file", () => {
    renderPage();
    paste(contents()[1], "API_KEY=abc123\nOTHER=1\nBACKUP_KEY=abc123\n");
    expect(screen.getByText(/BACKUP_KEY has the same value as API_KEY on line 1/)).toBeDefined();
  });

  it("loads a file by path, and saves edits back only after confirming", async () => {
    invoke.mockImplementation(async (cmd: string) =>
      cmd === "env_diff_read_file" ? "A=1\n" : "Saved /work/api/.env",
    );
    renderPage();

    fireEvent.change(screen.getAllByLabelText("File")[0], { target: { value: "/work/api/.env" } });
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button", { name: "Load" })[0]);
    });
    await waitFor(() => expect(contents()[0].value).toBe("A=1\n"));
    expect(invoke).toHaveBeenCalledWith("env_diff_read_file", { path: "/work/api/.env" });
    expect(screen.queryByRole("button", { name: "Save to file" })).toBeNull();

    paste(contents()[0], "A=1\nB=2\n");
    fireEvent.click(screen.getByRole("button", { name: "Save to file" }));
    // Nothing is written until the confirmation is accepted.
    expect(invoke).not.toHaveBeenCalledWith("env_diff_write_file", expect.anything());

    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    });
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("env_diff_write_file", {
        path: "/work/api/.env",
        contents: "A=1\nB=2\n",
      }),
    );
    await waitFor(() => expect(screen.queryByRole("button", { name: "Save to file" })).toBeNull());
  });

  it("surfaces a read failure instead of leaving the box silently empty", async () => {
    invoke.mockRejectedValue("could not read /nope: No such file or directory");
    renderPage();
    fireEvent.change(screen.getAllByLabelText("File")[1], { target: { value: "/nope" } });
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button", { name: "Load" })[1]);
    });
    expect(await screen.findByText(/No such file or directory/)).toBeDefined();
  });
});

describe("EnvDiffPage sessions", () => {
  const FILES: Record<string, string> = {
    "/w/api/.env": "A=1\nSECRET=hunter2\n",
    "/w/api/.env.example": "A=1\nNEW=x\n",
  };
  const SAVED = {
    name: "api",
    left: "/w/api/.env",
    right: "/w/api/.env.example",
    keysOnly: false,
    syncScroll: true,
    filter: "all",
  };
  const withStored = (list: unknown[]) =>
    invoke.mockImplementation(async (cmd: string, args: { path?: string }) => {
      if (cmd === "settings_get") return JSON.stringify(list);
      if (cmd === "env_diff_read_file") {
        if (args.path && args.path in FILES) return FILES[args.path];
        throw `could not read ${args.path}: No such file or directory`;
      }
      return undefined;
    });
  const openFromList = async (name: string) => {
    fireEvent.click(screen.getByRole("button", { name: "Sessions" }));
    const row = (await screen.findByText(name)).closest("tr") as HTMLElement;
    await act(async () => {
      fireEvent.click(within(row).getByRole("button", { name: "Open" }));
    });
  };

  it("saves the loaded file paths and options, never the contents", async () => {
    withStored([]);
    renderPage();
    fireEvent.change(screen.getAllByLabelText("File")[0], { target: { value: "/w/api/.env" } });
    await act(async () => {
      fireEvent.click(screen.getAllByRole("button", { name: "Load" })[0]);
    });
    await waitFor(() => expect(contents()[0].value).toBe(FILES["/w/api/.env"]));
    paste(contents()[1], "PASTED=topsecret\n");

    fireEvent.click(screen.getByRole("button", { name: "Save session" }));
    expect((screen.getByLabelText("Name") as HTMLInputElement).value).toBe("api/.env");
    expect(screen.getByText("pasted text, not saved")).toBeDefined();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
    });

    const call = invoke.mock.calls.find(([cmd]) => cmd === "settings_set");
    const { key, value } = call?.[1] as { key: string; value: string };
    expect(key).toBe("env-diff.sessions");
    expect(JSON.parse(value)).toEqual([
      { name: "api/.env", left: "/w/api/.env", right: null, keysOnly: false, syncScroll: true, filter: "diff" },
    ]);
    expect(value).not.toContain("hunter2");
    expect(value).not.toContain("topsecret");
    expect(await screen.findByText('Saved session "api/.env".')).toBeDefined();
  });

  it("opens a saved session by reading its files from disk again", async () => {
    withStored([SAVED]);
    renderPage();
    await openFromList("api");

    await waitFor(() => expect(contents()[0].value).toBe(FILES["/w/api/.env"]));
    expect(contents()[1].value).toBe(FILES["/w/api/.env.example"]);
    expect(screen.getByText('Opened session "api".')).toBeDefined();
    // The saved "all" filter came with it, so the matching variable shows too.
    expect(within(table()).getByText("A")).toBeDefined();
  });

  it("asks before a session replaces text that isn't in a file", async () => {
    withStored([SAVED]);
    renderPage();
    paste(contents()[0], "PASTED=1\n");
    await openFromList("api");

    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Discard and open" })).toBeNull(),
    );
    expect(contents()[0].value).toBe("PASTED=1\n");
    expect(invoke).not.toHaveBeenCalledWith("env_diff_read_file", expect.anything());
  });

  it("leaves a file it can't read in the path box, ready to fix", async () => {
    withStored([{ ...SAVED, right: "/w/gone/.env" }]);
    renderPage();
    await openFromList("api");

    expect(await screen.findByText(/a file couldn't be read: could not read \/w\/gone\/\.env/)).toBeDefined();
    expect(contents()[0].value).toBe(FILES["/w/api/.env"]);
    const pathBoxes = screen.getAllByLabelText("File") as HTMLInputElement[];
    expect(pathBoxes[pathBoxes.length - 1].value).toBe("/w/gone/.env");
  });

  it("deletes a saved session", async () => {
    withStored([SAVED]);
    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "Sessions" }));
    const row = (await screen.findByText("api")).closest("tr") as HTMLElement;
    await act(async () => {
      fireEvent.click(within(row).getByRole("button", { name: "Delete api" }));
    });
    expect(invoke).toHaveBeenCalledWith("settings_set", { key: "env-diff.sessions", value: "[]" });
    expect(screen.getByText("No saved sessions")).toBeDefined();
  });
});
