import { describe, expect, it } from "vitest";
import { normalisePath, parseFavourites, withFavourite, withoutFavourite } from "./favourites";

describe("parseFavourites", () => {
  it("reads back what was stored", () => {
    const stored = { "host-a": ["/etc/nginx", "/srv/app"], "host-b": ["/"] };
    expect(parseFavourites(JSON.stringify(stored))).toEqual(stored);
  });

  it("returns nothing for missing or unreadable storage", () => {
    expect(parseFavourites(null)).toEqual({});
    expect(parseFavourites(undefined)).toEqual({});
    expect(parseFavourites("not json")).toEqual({});
    expect(parseFavourites('["/srv"]')).toEqual({});
    expect(parseFavourites("42")).toEqual({});
  });

  it("drops junk and relative paths, and tidies the rest", () => {
    expect(
      parseFavourites(
        JSON.stringify({
          a: ["/srv/app/", 7, "relative/dir", "/srv/app", "/etc", ""],
          b: "not a list",
          c: [null, "logs"],
        }),
      ),
    ).toEqual({ a: ["/etc", "/srv/app"] });
  });
});

describe("normalisePath", () => {
  it("drops trailing slashes but keeps the root", () => {
    expect(normalisePath("/srv/app/")).toBe("/srv/app");
    expect(normalisePath("/srv/app//")).toBe("/srv/app");
    expect(normalisePath("/")).toBe("/");
    expect(normalisePath("//")).toBe("/");
  });
});

describe("withFavourite", () => {
  it("adds per host, once, with each folder above its subfolders", () => {
    let all = withFavourite({}, "a", "/srv2");
    all = withFavourite(all, "a", "/srv/app");
    all = withFavourite(all, "a", "/srv");
    all = withFavourite(all, "a", "/srv/app/");
    all = withFavourite(all, "b", "/srv");
    expect(all).toEqual({ a: ["/srv", "/srv/app", "/srv2"], b: ["/srv"] });
  });
});

describe("withoutFavourite", () => {
  it("removes from one host only, and drops a host left with none", () => {
    const all = { a: ["/etc", "/srv"], b: ["/srv"] };
    expect(withoutFavourite(all, "a", "/srv/")).toEqual({ a: ["/etc"], b: ["/srv"] });
    expect(withoutFavourite(all, "b", "/srv")).toEqual({ a: ["/etc", "/srv"] });
    expect(withoutFavourite(all, "nope", "/srv")).toEqual(all);
  });
});
