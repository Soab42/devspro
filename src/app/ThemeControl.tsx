import { SegmentedControl } from "../ui";
import { Monitor, Moon, Sun } from "../ui/icons";
import { Theme, useTheme } from "./ThemeContext";

const OPTIONS = [
  { value: "light" as Theme, label: "Light", icon: Sun },
  { value: "system" as Theme, label: "System", icon: Monitor },
  { value: "dark" as Theme, label: "Dark", icon: Moon },
];

export interface ThemeControlProps {
  /** Icon-only in the topbar; labelled in Settings where there is room. */
  iconOnly?: boolean;
  size?: "sm" | "md";
}

/**
 * One control, two mount points. The topbar and the Settings page previously
 * each hand-rolled their own three-button toggle over the same state, so a
 * change to one silently diverged from the other.
 */
export function ThemeControl({ iconOnly = true, size = "sm" }: ThemeControlProps) {
  const { theme, setTheme } = useTheme();
  return (
    <SegmentedControl
      ariaLabel="Colour theme"
      options={OPTIONS}
      value={theme}
      onChange={setTheme}
      iconOnly={iconOnly}
      size={size}
    />
  );
}
