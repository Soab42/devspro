import { describe, expect, it } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { ConfirmProvider, useConfirm } from "./ConfirmContext";

/**
 * ConfirmContext guards ~12 destructive actions (force-kill, delete database
 * and its volume, delete site, remove worker…). A dismissal path that resolves
 * `true` by accident is a data-loss bug, so every one of them is pinned here.
 */
function Harness({ onResult }: { onResult: (v: boolean) => void }) {
  const confirm = useConfirm();
  return (
    <button
      onClick={async () => {
        const result = await confirm("Delete everything?", {
          title: "Delete database?",
          confirmLabel: "Delete",
          danger: true,
        });
        onResult(result);
      }}
    >
      open
    </button>
  );
}

function setup() {
  const results: boolean[] = [];
  render(
    <ConfirmProvider>
      <Harness onResult={(v) => results.push(v)} />
    </ConfirmProvider>,
  );
  return results;
}

async function open() {
  await act(async () => {
    screen.getByRole("button", { name: "open" }).click();
  });
}

describe("ConfirmContext", () => {
  it("resolves true only via the confirming action", async () => {
    const results = setup();
    await open();
    await act(async () => {
      screen.getByRole("button", { name: "Delete" }).click();
    });
    await waitFor(() => expect(results).toEqual([true]));
  });

  it("resolves false on Cancel", async () => {
    const results = setup();
    await open();
    await act(async () => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    await waitFor(() => expect(results).toEqual([false]));
  });

  it("resolves false on Escape", async () => {
    const results = setup();
    await open();
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    await waitFor(() => expect(results).toEqual([false]));
  });

  it("resolves false on a backdrop click", async () => {
    const results = setup();
    await open();
    const overlay = document.querySelector(".modal-overlay");
    expect(overlay).not.toBeNull();
    await act(async () => {
      overlay!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });
    await waitFor(() => expect(results).toEqual([false]));
  });

  it("resolves false via the close button", async () => {
    const results = setup();
    await open();
    await act(async () => {
      screen.getByRole("button", { name: "Close" }).click();
    });
    await waitFor(() => expect(results).toEqual([false]));
  });

  it("does not autofocus the confirming action when destructive", async () => {
    setup();
    await open();
    // A stray Enter must not be able to trigger a delete.
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Delete" }));
  });

  it("uses the danger styling for destructive prompts", async () => {
    setup();
    await open();
    expect(screen.getByRole("button", { name: "Delete" }).className).toContain(
      "btn-danger-solid",
    );
  });
});
