import { useEffect, useState } from "react";

/**
 * False while the window is minimised, hidden to the tray or behind another
 * workspace — anything the browser reports as `document.hidden`. Polling hooks
 * pass it as `enabled` so a page nobody can see costs nothing.
 */
export function useDocumentVisible(): boolean {
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden");
  useEffect(() => {
    const update = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  return visible;
}
