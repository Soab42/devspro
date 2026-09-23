import { describe, expect, it } from "vitest";
import {
  connectionTarget,
  customIdOf,
  customRef,
  dumpFileName,
  generatePassword,
  hasNextPage,
  isBrowsable,
  isCustomRef,
  isEngineImage,
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

describe("connection refs", () => {
  it("round-trips a saved connection id through a ref", () => {
    const ref = customRef("abc-123");
    expect(ref).toBe("custom:abc-123");
    expect(isCustomRef(ref)).toBe(true);
    expect(customIdOf(ref)).toBe("abc-123");
  });

  it("treats a bare compose service name as managed, not custom", () => {
    expect(isCustomRef("myapp_db")).toBe(false);
    expect(customIdOf("myapp_db")).toBeNull();
  });
});

describe("connectionTarget", () => {
  it("shows host and port for a TCP connection", () => {
    expect(
      connectionTarget({
        transport: "tcp",
        host: "db.example.com",
        port: 3306,
        container: "",
      }),
    ).toBe("db.example.com:3306");
  });

  it("shows the container for a docker connection, which has no host", () => {
    expect(
      connectionTarget({
        transport: "docker",
        host: "",
        port: 0,
        container: "app_mysql_1",
      }),
    ).toBe("app_mysql_1");
  });
});

describe("isEngineImage", () => {
  it("matches the official images for each engine", () => {
    expect(isEngineImage("mysql:8", "mysql")).toBe(true);
    expect(isEngineImage("mariadb:11", "mysql")).toBe(true);
    expect(isEngineImage("postgres:16-alpine", "postgres")).toBe(true);
  });

  it("looks past a registry prefix and a tag", () => {
    expect(isEngineImage("ghcr.io/acme/postgres:16", "postgres")).toBe(true);
    expect(isEngineImage("docker.io/library/MySQL:8", "mysql")).toBe(true);
  });

  it("keeps the two engines apart", () => {
    expect(isEngineImage("postgres:16", "mysql")).toBe(false);
    expect(isEngineImage("mysql:8", "postgres")).toBe(false);
  });

  it("does not match an ordinary application container", () => {
    expect(isEngineImage("brokercore-deal", "mysql")).toBe(false);
    expect(isEngineImage("redis:7-alpine", "postgres")).toBe(false);
  });
});
