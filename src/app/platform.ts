/**
 * Which OS the app is running on, for the few places the UI speaks in that
 * OS's terms — an install command, an example path. Behaviour that differs by
 * OS belongs in the backend; this is only for words.
 *
 * Read from the user agent: WebView2 reports "Windows NT" on every Windows
 * build, and `navigator.platform` is deprecated.
 */
export const isWindows =
  typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent);

/** An example absolute path under the user's home folder, for placeholders. */
export function examplePath(...parts: string[]): string {
  return isWindows
    ? ["C:\\Users\\you", ...parts].join("\\")
    : ["/home/you", ...parts].join("/");
}
