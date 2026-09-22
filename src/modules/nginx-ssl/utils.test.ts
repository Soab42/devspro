import { describe, expect, it } from "vitest";
import {
  connectionOption,
  dbNameSlug,
  plannedSteps,
  planSummary,
  slugForHostname,
  splitProjectFolder,
  stepState,
  suggestDomain,
  type WizardPlan,
} from "./utils";

const base: WizardPlan = {
  name: "shop",
  parent_dir: "/home/me/code/",
  template: "laravel",
  domain: "shop.test",
  php_version: null,
  create_database: false,
  db_connection: null,
  db_name: null,
  write_env: false,
  run_migrations: false,
  open_in_editor: null,
};

describe("suggestDomain", () => {
  it("slugs the name and appends .test", () => {
    expect(slugForHostname("My Shop!")).toBe("my-shop");
    expect(suggestDomain("My Shop!")).toBe("my-shop.test");
    expect(suggestDomain("acme_api")).toBe("acme-api.test");
  });
  it("is empty when nothing usable remains", () => {
    expect(suggestDomain("")).toBe("");
    expect(suggestDomain("!!!")).toBe("");
  });
});

describe("dbNameSlug", () => {
  it("produces a valid SQL identifier", () => {
    expect(dbNameSlug("My Shop")).toBe("my_shop");
    expect(dbNameSlug("shop-api-2")).toBe("shop_api_2");
    expect(dbNameSlug("2fast")).toBe("fast");
    expect(dbNameSlug("shop--")).toBe("shop");
  });
});

describe("splitProjectFolder", () => {
  it("splits into parent and basename, tolerating a trailing slash", () => {
    expect(splitProjectFolder("/home/me/code/shop")).toEqual({ parentDir: "/home/me/code", name: "shop" });
    expect(splitProjectFolder("/home/me/code/shop/")).toEqual({ parentDir: "/home/me/code", name: "shop" });
    expect(splitProjectFolder("/shop")).toEqual({ parentDir: "/", name: "shop" });
    expect(splitProjectFolder("shop")).toEqual({ parentDir: "", name: "shop" });
  });
});

describe("connectionOption", () => {
  const container = (name: string, image: string, state = "running") => ({
    id: "x",
    name,
    image,
    state,
    status: "",
    ports: "",
  });
  it("strips the devsuite prefix and labels with the image", () => {
    expect(connectionOption(container("devsuite_mysql8", "mysql:8"))).toEqual({
      value: "mysql8",
      label: "mysql8 — mysql:8",
    });
    expect(connectionOption(container("devsuite_pg", "postgres:16", "exited"))?.label).toBe(
      "pg — postgres:16 (exited)",
    );
  });
  it("excludes engines Laravel cannot use as DB_CONNECTION", () => {
    expect(connectionOption(container("devsuite_cache", "redis:7"))).toBeNull();
    expect(connectionOption(container("devsuite_ui", "phpmyadmin:latest"))).toBeNull();
  });
});

describe("plannedSteps", () => {
  it("always has the three base steps and adds the optional ones in order", () => {
    expect(plannedSteps(base)).toEqual(["scaffold", "site", "hosts"]);
    expect(
      plannedSteps({
        ...base,
        create_database: true,
        write_env: true,
        run_migrations: true,
        open_in_editor: "code",
      }),
    ).toEqual(["scaffold", "site", "hosts", "database", "env", "artisan", "editor"]);
    expect(plannedSteps({ ...base, open_in_editor: "  " })).not.toContain("editor");
  });
});

describe("planSummary", () => {
  it("renders one row per decision with the folder joined", () => {
    const rows = planSummary({ ...base, create_database: true, db_connection: "mysql8", db_name: "shop" });
    expect(rows.find((r) => r.label === "Folder")?.value).toBe("/home/me/code/shop");
    expect(rows.find((r) => r.label === "Domain")?.value).toBe("https://shop.test");
    expect(rows.find((r) => r.label === "PHP")?.value).toBe("System default");
    expect(rows.find((r) => r.label === "Database")?.value).toBe("create shop on mysql8");
    expect(rows.find((r) => r.label === "Editor")).toBeUndefined();
  });
  it("adds the editor row only when set", () => {
    const rows = planSummary({ ...base, open_in_editor: "code" });
    expect(rows[rows.length - 1]).toEqual({ label: "Editor", value: "code" });
  });
});

describe("stepState", () => {
  const steps = ["scaffold", "site", "hosts"];
  it("is pending before a run starts", () => {
    expect(stepState("scaffold", steps, null, false)).toBe("pending");
  });
  it("marks the first unreported step as running while the command is in flight", () => {
    expect(stepState("scaffold", steps, null, true)).toBe("running");
    expect(stepState("site", steps, null, true)).toBe("pending");
  });
  it("reads results as ok/bad and later steps as skipped after a failure", () => {
    const results = [
      { step: "scaffold", ok: true, message: "" },
      { step: "site", ok: false, message: "nope" },
    ];
    expect(stepState("scaffold", steps, results, false)).toBe("ok");
    expect(stepState("site", steps, results, false)).toBe("bad");
    expect(stepState("hosts", steps, results, false)).toBe("skipped");
  });
});
