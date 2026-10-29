import { describe, expect, it } from "vitest";
import type { Device } from "./api";
import { breadcrumb, deviceAt, friendlyPath } from "./devices";

const GVFS = "/run/user/1000/gvfs";
const device = (over: Partial<Device>): Device => ({
  id: "x",
  label: "X",
  kind: "usb",
  path: null,
  mount: null,
  unmount: null,
  eject: null,
  ...over,
});

const phone = device({ id: "phone", label: "Redmi Note 13", kind: "phone", path: `${GVFS}/mtp:host=Redmi` });
const drive = device({ id: "drive", label: "me@gmail.com", kind: "cloud", path: `${GVFS}/google-drive:host=gmail.com,user=me` });
const stick = device({ id: "stick", label: "Stick", path: null, mount: "device:/dev/sdb1" });
const list = [phone, drive, stick];

describe("devices", () => {
  it("finds the mounted device a path is on", () => {
    expect(deviceAt(`${phone.path}/Internal shared storage/DCIM`, list)?.id).toBe("phone");
    expect(deviceAt(phone.path!, list)?.id).toBe("phone");
    expect(deviceAt(`${phone.path}2/x`, list)).toBeNull();
    expect(deviceAt("/home/me", list)).toBeNull();
  });

  it("starts the path bar at the device, with titles in place of IDs", () => {
    const crumbs = breadcrumb(`${drive.path}/0AF5/1xz`, list, { [`${drive.path}/0AF5`]: "My Drive" });
    expect(crumbs.map((c) => c.label)).toEqual(["me@gmail.com", "My Drive", "1xz"]);
    expect(crumbs[2].path).toBe(`${drive.path}/0AF5/1xz`);
    expect(breadcrumb("/var/log", list, {}).map((c) => c.label)).toEqual(["/", "var", "log"]);
  });

  it("reads a path from the device's name, or from home", () => {
    expect(friendlyPath(`${phone.path}/Internal shared storage`, list, {}, "/home/me")).toBe("Redmi Note 13/Internal shared storage");
    expect(friendlyPath("/home/me/src", list, {}, "/home/me")).toBe("~/src");
  });

  it("puts Windows drives in the path bar, but keeps home ~-relative", () => {
    const c = device({ id: "C:", label: "Local Disk (C:)", kind: "drive", path: "C:\\" });
    const e = device({ id: "E:", label: "Stick (E:)", path: "E:\\" });
    const drives = [c, e];
    expect(deviceAt("E:\\photos", drives)?.id).toBe("E:");
    expect(breadcrumb("E:\\photos\\2024", drives, {}).map((x) => x.path)).toEqual(["E:\\", "E:\\photos", "E:\\photos\\2024"]);
    expect(friendlyPath("E:\\photos", drives, {}, "C:\\Users\\me")).toBe("Stick (E:)\\photos");
    expect(friendlyPath("C:\\Users\\me\\src", drives, {}, "C:\\Users\\me")).toBe("~\\src");
    expect(friendlyPath("C:\\Windows", drives, {}, "C:\\Users\\me")).toBe("Local Disk (C:)\\Windows");
  });
});
