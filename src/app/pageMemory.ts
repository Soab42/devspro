import { useEffect, useState, type Dispatch, type SetStateAction } from "react";

/**
 * `useState` that a page gets back when it is opened again: what it held when
 * the user left — two .env files being compared, the tab they were on — is
 * where they come back to, instead of a blank page.
 *
 * In memory only, for as long as DevsPro is open. Nothing runs in the
 * background because of it; it is the page's state, not a session.
 */
const memory = new Map<string, unknown>();

export function useRemembered<T>(key: string, initial: T | (() => T)): [T, Dispatch<SetStateAction<T>>] {
  const [value, setValue] = useState<T>(() =>
    memory.has(key) ? (memory.get(key) as T) : typeof initial === "function" ? (initial as () => T)() : initial,
  );
  useEffect(() => {
    memory.set(key, value);
  }, [key, value]);
  return [value, setValue];
}

/** Forgets every page's remembered state. For tests, which render a page afresh each time. */
export function forgetRememberedState() {
  memory.clear();
}
