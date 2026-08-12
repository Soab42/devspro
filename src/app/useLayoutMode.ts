import { useEffect, useState } from "react";

/**
 * How the navigation should present itself at the current window width.
 *
 *   full   — labelled sidebar
 *   rail   — icons only, still always visible
 *   drawer — hidden behind a toggle, overlays the content
 *
 * The middle state is the one the old CSS lacked: it jumped straight from a
 * 224px sidebar to an off-canvas drawer at 860px, so a half-width window lost
 * its navigation entirely.
 */
export type LayoutMode = "full" | "rail" | "drawer";

/** Matches the rem breakpoints; converted to px against the root font size. */
const RAIL_BELOW_REM = 60;
const DRAWER_BELOW_REM = 40;

function rootFontSize(): number {
  return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
}

function measure(): LayoutMode {
  const rem = rootFontSize();
  const width = window.innerWidth;
  if (width < DRAWER_BELOW_REM * rem) return "drawer";
  if (width < RAIL_BELOW_REM * rem) return "rail";
  return "full";
}

export function useLayoutMode(): LayoutMode {
  const [mode, setMode] = useState<LayoutMode>(measure);

  useEffect(() => {
    // Measured in JS rather than with CSS media queries because the root font
    // size is itself fluid (clamp on vw), so rem breakpoints do not map to
    // fixed px values — a media query written in rem would drift against the
    // very layout it is meant to control.
    const onResize = () => setMode(measure());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return mode;
}
