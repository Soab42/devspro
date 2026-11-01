import { describe, expect, it } from "vitest";
import { JSDOM } from "jsdom";
import html from "../../index.html?raw";

// index.html's own inline guard, run without the app: these tests are about
// what the window shows when the app fails before or after it draws.
function boot() {
  const page = html.replace(/<script type="module"[^>]*><\/script>/, "");
  const { window } = new JSDOM(page, { runScripts: "dangerously", pretendToBeVisual: true });
  return window;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const notice = (win: { document: Document }) => win.document.getElementById("boot-failure");

describe("the boot guard in index.html", () => {
  it("shows the error, with a Reload button, when the app throws before it draws", async () => {
    const win = boot();
    await sleep(20);
    win.dispatchEvent(
      new win.ErrorEvent("error", {
        message: "ReferenceError: LibraryBig is not defined",
        filename: "http://localhost:1420/src/app/ModuleRegistry.ts",
        lineno: 3,
        colno: 9,
      }),
    );
    await sleep(500);
    const box = notice(win);
    expect(box?.getAttribute("role")).toBe("alert");
    expect(box?.querySelector("pre")?.textContent).toContain("LibraryBig is not defined (http://localhost:1420/src/app/ModuleRegistry.ts:3:9)");
    expect([...(box?.querySelectorAll("button") ?? [])].map((b) => b.textContent)).toEqual(["Reload", "Copy details"]);
  });

  it("leaves a running app alone when something it drew throws", async () => {
    const win = boot();
    await sleep(20);
    win.document.getElementById("root")?.appendChild(win.document.createElement("main"));
    await sleep(20);
    win.dispatchEvent(new win.ErrorEvent("error", { message: "boom" }));
    await sleep(500);
    expect(notice(win)).toBeNull();
  });

  it("speaks up when the app it drew disappears", async () => {
    const win = boot();
    await sleep(20);
    const root = win.document.getElementById("root") as HTMLElement;
    root.appendChild(win.document.createElement("main"));
    await sleep(20);
    root.replaceChildren();
    await sleep(500);
    expect(notice(win)?.querySelector("pre")?.textContent).toBe("No error was reported; the interface was removed.");

    // The app drawing again takes the notice away.
    root.appendChild(win.document.createElement("main"));
    await sleep(20);
    expect(notice(win)).toBeNull();
  });

  it("ignores an image that failed to load", async () => {
    const win = boot();
    await sleep(20);
    const img = win.document.createElement("img");
    win.document.body.appendChild(img);
    img.dispatchEvent(new win.Event("error"));
    await sleep(500);
    expect(notice(win)).toBeNull();
  });
});
