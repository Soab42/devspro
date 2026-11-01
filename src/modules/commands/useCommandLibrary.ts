import { useCallback, useEffect, useMemo, useState } from "react";
import { useAuth } from "../../auth/AuthContext";
import { getSetting, setSetting } from "../settings/api";
import { loadGlobalCommands, parsePersonal, personalKey, personalToLibrary } from "./library";
import type { LibraryCommand, PersonalCommand } from "./types";

/** Fired after a save, so the page and the terminal panel stay in step. */
const CHANGED = "devspro:commands-changed";

// One load of the catalogue per window; it doesn't change while the app runs.
let globalPromise: Promise<LibraryCommand[]> | null = null;

export interface CommandLibrary {
  /** Global first, then personal. Empty until the first load finishes. */
  commands: LibraryCommand[];
  personal: PersonalCommand[];
  loading: boolean;
  error: string | null;
  /** Adds the command, or replaces the one with its id. */
  savePersonal: (command: PersonalCommand) => Promise<void>;
  removePersonal: (id: string) => Promise<void>;
  /** Category names already in use, for the add form to suggest. */
  categories: string[];
}

export function useCommandLibrary(): CommandLibrary {
  const { profile } = useAuth();
  const key = profile ? personalKey(profile.id) : null;
  const [global, setGlobal] = useState<LibraryCommand[]>([]);
  const [personal, setPersonal] = useState<PersonalCommand[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    globalPromise ??= loadGlobalCommands();
    globalPromise.then(
      (commands) => live && setGlobal(commands),
      (e) => {
        globalPromise = null;
        if (live) setError(`Could not load the command library: ${String(e)}`);
      },
    );
    return () => {
      live = false;
    };
  }, []);

  const loadPersonal = useCallback(async () => {
    if (!key) return setPersonal([]);
    setPersonal(parsePersonal(await getSetting(key).catch(() => null)));
  }, [key]);

  useEffect(() => {
    let live = true;
    setLoading(true);
    loadPersonal().finally(() => live && setLoading(false));
    const reload = () => void loadPersonal();
    window.addEventListener(CHANGED, reload);
    return () => {
      live = false;
      window.removeEventListener(CHANGED, reload);
    };
  }, [loadPersonal]);

  const write = useCallback(
    async (next: PersonalCommand[]) => {
      if (!key) throw new Error("No profile is unlocked.");
      await setSetting(key, JSON.stringify(next));
      setPersonal(next);
      window.dispatchEvent(new Event(CHANGED));
    },
    [key],
  );

  const savePersonal = useCallback(
    (command: PersonalCommand) =>
      write(personal.some((c) => c.id === command.id) ? personal.map((c) => (c.id === command.id ? command : c)) : [...personal, command]),
    [personal, write],
  );
  const removePersonal = useCallback((id: string) => write(personal.filter((c) => c.id !== id)), [personal, write]);

  const commands = useMemo(() => [...global, ...personalToLibrary(personal)], [global, personal]);
  const categories = useMemo(
    () => [...new Set([...personal.map((c) => c.category), ...global.map((c) => c.category)])],
    [global, personal],
  );

  return {
    commands,
    personal,
    loading: loading || (global.length === 0 && !error),
    error,
    savePersonal,
    removePersonal,
    categories,
  };
}
