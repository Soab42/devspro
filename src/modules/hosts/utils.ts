/**
 * Client-side checks for the add/edit form. The backend validates again and is
 * the authority; these exist so a typo is flagged before the password prompt
 * appears, which is the expensive part of every write.
 */

const IPV4 = /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}$/;
// Loose: hex groups and colons, with at most one "::". Full IPv6 grammar is the
// backend's job.
const IPV6 = /^[0-9a-fA-F:.]+$/;

export function isValidIp(ip: string): boolean {
  const s = ip.trim();
  if (IPV4.test(s)) return true;
  if (!s.includes(":") || !IPV6.test(s)) return false;
  return s.split("::").length <= 2;
}

const LABEL = /^[A-Za-z0-9_]([A-Za-z0-9_-]*[A-Za-z0-9_])?$/;

export function isValidHostname(hostname: string): boolean {
  const s = hostname.trim();
  if (s.length === 0 || s.length > 253) return false;
  return s.split(".").every((label) => label.length <= 63 && LABEL.test(label));
}

/** Splits a free-text hostnames field on whitespace or commas, dropping blanks. */
export function splitHostnames(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((h) => h.trim())
    .filter((h) => h.length > 0);
}

/** Error message for the form, or null when the fields are acceptable. */
export function validateEntry(ip: string, hostnames: string[]): string | null {
  if (!isValidIp(ip)) return "Enter a valid IPv4 or IPv6 address.";
  if (hostnames.length === 0) return "Enter at least one hostname.";
  const bad = hostnames.find((h) => !isValidHostname(h));
  if (bad) return `"${bad}" is not a valid hostname.`;
  return null;
}
