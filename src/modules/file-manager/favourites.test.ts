import { describe, expect, it } from "vitest";
import { parseFavourites } from "./favourites";

describe("parseFavourites", () => {
  it("reads a saved list and fills in missing labels", () => {
    expect(parseFavourites('[{"path":"/home/me/app","label":"App"},{"path":"/var/log"}]')).toEqual([
      { path: "/home/me/app", label: "App" },
      { path: "/var/log", label: "log" },
    ]);
  });

  it("ignores junk", () => {
    expect(parseFavourites(null)).toEqual([]);
    expect(parseFavourites("not json")).toEqual([]);
    expect(parseFavourites('{"path":"/x"}')).toEqual([]);
    expect(parseFavourites('[1, {"label":"no path"}, {"path":"/"}]')).toEqual([{ path: "/", label: "/" }]);
  });
});
