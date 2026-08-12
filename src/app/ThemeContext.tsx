import { createContext, ReactNode, useContext, useEffect, useState } from "react";

export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "devsuite:theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

interface ThemeState {
  /** What the user chose, including "system". */
  theme: Theme;
  /** What that actually resolves to right now. Always concrete. */
  resolved: ResolvedTheme;
  setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeState | null>(null);

function readStoredTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "light" || stored === "dark" ? stored : "system";
}

function systemTheme(): ResolvedTheme {
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function resolve(theme: Theme): ResolvedTheme {
  return theme === "system" ? systemTheme() : theme;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);
  const [resolved, setResolved] = useState<ResolvedTheme>(() => resolve(readStoredTheme()));

  // We always write a concrete light/dark onto the root, never remove the
  // attribute. That is what lets tokens.css express dark mode as a single
  // `:root[data-theme="dark"]` block instead of duplicating the whole palette
  // into a `prefers-color-scheme` media query as well.
  //
  // The matching pre-paint script in index.html applies the same rule before
  // the bundle loads, so this effect is normally a no-op on first render.
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", resolved);
  }, [resolved]);

  useEffect(() => {
    setResolved(resolve(theme));
    if (theme !== "system") return;

    // Only while following the system do we need to track OS changes.
    const media = window.matchMedia(DARK_QUERY);
    const onChange = () => setResolved(systemTheme());
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [theme]);

  function setTheme(next: Theme) {
    setThemeState(next);
    if (next === "system") {
      localStorage.removeItem(STORAGE_KEY);
    } else {
      localStorage.setItem(STORAGE_KEY, next);
    }
  }

  return (
    <ThemeContext.Provider value={{ theme, resolved, setTheme }}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeState {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
