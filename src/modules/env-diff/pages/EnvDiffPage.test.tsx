import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ConfirmProvider } from "../../../app/ConfirmContext";
import EnvDiffPage from "./EnvDiffPage";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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
