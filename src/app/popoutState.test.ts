import { describe, expect, it } from "vitest";
import { currentPopoutHandoff, registerPopoutHandoff } from "./popoutState";

describe("popout hand-off", () => {
  it("is empty until a page registers, and empty again once it leaves", () => {
    expect(currentPopoutHandoff()).toBeNull();
    const unregister = registerPopoutHandoff(() => ({ params: { path: "/tmp" } }));
    expect(currentPopoutHandoff()).toEqual({ params: { path: "/tmp" } });
    unregister();
    expect(currentPopoutHandoff()).toBeNull();
  });

  it("reads the page's latest state each time it is asked", () => {
    let path = "/a";
    const unregister = registerPopoutHandoff(() => ({ params: { path } }));
    path = "/b";
    expect(currentPopoutHandoff()?.params).toEqual({ path: "/b" });
    unregister();
  });

  it("does not let a stale unregister remove the page that replaced it", () => {
    const first = registerPopoutHandoff(() => ({ params: { n: "1" } }));
    registerPopoutHandoff(() => ({ params: { n: "2" } }));
    first();
    expect(currentPopoutHandoff()?.params).toEqual({ n: "2" });
  });
});
