import { describe, expect, it } from "vitest";
import {
  cellSortValue,
  dumpFileName,
  generatePassword,
  hasNextPage,
  isBrowsable,
  orderByFrom,
  pageLabel,
} from "./utils";

describe("generatePassword", () => {
  it("defaults to 20 characters", () => {
    expect(generatePassword()).toHaveLength(20);
  });

  it("respects a custom length", () => {
    expect(generatePassword(8)).toHaveLength(8);
    expect(generatePassword(64)).toHaveLength(64);
  });

  it("only uses characters from the declared charset", () => {
    const charset =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_!@#%^*+=";
    const password = generatePassword(200);
    for (const char of password) {
      expect(charset).toContain(char);
    }
  });

  it("is not trivially constant across calls", () => {
    const passwords = new Set(
      Array.from({ length: 5 }, () => generatePassword()),
    );
    expect(passwords.size).toBeGreaterThan(1);
  });
});

describe("pageLabel", () => {
  it("shows a range with the total when known", () => {
    expect(pageLabel(0, 50, 1234)).toBe("1–50 of 1,234");
    expect(pageLabel(1200, 34, 1234)).toBe("1,201–1,234 of 1,234");
  });

  it("omits the total for ad-hoc queries", () => {
    expect(pageLabel(0, 3, null)).toBe("1–3");
  });

  it("says so when there is nothing", () => {
    expect(pageLabel(0, 0, 0)).toBe("No rows");
    expect(pageLabel(0, 0, null)).toBe("No rows");
  });
});

describe("hasNextPage", () => {
  it("uses the total when it is known", () => {
    expect(hasNextPage(0, 50, 120)).toBe(true);
    expect(hasNextPage(100, 20, 120)).toBe(false);
  });

  it("falls back to a full page when the total is unknown", () => {
    expect(hasNextPage(0, 50, null)).toBe(true);
    expect(hasNextPage(0, 12, null)).toBe(false);
  });
});

describe("orderByFrom", () => {
  it("maps table sort state to the backend's order_by string", () => {
    expect(orderByFrom({ key: "id", dir: "desc" })).toBe("id desc");
    expect(orderByFrom(undefined)).toBeUndefined();
  });
});

describe("cellSortValue", () => {
  it("compares numeric text numerically and NULL first", () => {
    expect(cellSortValue("10")).toBe(10);
    expect(cellSortValue("abc")).toBe("abc");
    expect(cellSortValue("")).toBe("");
    expect(cellSortValue("NULL")).toBe("");
  });
});

describe("isBrowsable", () => {
  it("only MySQL-family and Postgres images are browsable", () => {
    expect(isBrowsable("mysql:8")).toBe(true);
    expect(isBrowsable("postgres:16")).toBe(true);
    expect(isBrowsable("mongo:7")).toBe(false);
    expect(isBrowsable("redis:7")).toBe(false);
  });
});

describe("dumpFileName", () => {
  it("dates the file and uses a .sql extension", () => {
    expect(dumpFileName("shop", new Date(2026, 8, 22))).toBe(
      "shop_20260922.sql",
    );
  });
});
