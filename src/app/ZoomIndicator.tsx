import { useEffect, useState } from "react";
import { getZoom, subscribeZoom } from "./zoom";
import "./ZoomIndicator.css";

const VISIBLE_MS = 1200;

/** A brief "110%" in the corner whenever the zoom changes. */
export function ZoomIndicator() {
  const [shown, setShown] = useState<number | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = subscribeZoom((announce) => {
      if (!announce) return;
      setShown(getZoom());
      clearTimeout(timer);
      timer = setTimeout(() => setShown(null), VISIBLE_MS);
    });
    return () => {
      off();
      clearTimeout(timer);
    };
  }, []);

  if (shown === null) return null;
  return (
    <div className="zoom-indicator" role="status" aria-live="polite">
      {shown}%
    </div>
  );
}
