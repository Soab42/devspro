import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { LAST_SCAN_KEY } from "../../cleaner/components/overview/lastScan";
import { CleanerCard } from "./CleanerCard";

const GB = 1024 * 1024 * 1024;

function show() {
  render(
    <MemoryRouter>
      <CleanerCard />
    </MemoryRouter>,
  );
}

beforeEach(() => localStorage.removeItem(LAST_SCAN_KEY));
afterEach(cleanup);

describe("CleanerCard", () => {
  it("invites a scan when none is saved, and links to the Cleaner", () => {
    show();
    const link = screen.getByRole("link", { name: "Scan for space you can free" });
    expect(link.getAttribute("href")).toBe("/cleaner");
  });

  it("shows the saved total and age, and links to the Cleaner", () => {
    const at = new Date(Date.now() - 2 * 3600 * 1000).toISOString();
    localStorage.setItem(LAST_SCAN_KEY, JSON.stringify({ at, totals: { safe: 2 * GB, review: GB, danger: 0 } }));
    show();
    const link = screen.getByRole("link", { name: /About 3\.0 GB can be freed, scanned 2 hours ago/ });
    expect(link.getAttribute("href")).toBe("/cleaner");
  });

  it("treats a garbled saved value as no scan", () => {
    localStorage.setItem(LAST_SCAN_KEY, "{nope");
    show();
    expect(screen.getByRole("link", { name: "Scan for space you can free" })).toBeTruthy();
  });
});
