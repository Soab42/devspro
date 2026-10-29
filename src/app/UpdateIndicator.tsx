import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Icon } from "../ui";
import { ArrowUpCircle } from "../ui/icons";
import { onUpdateAvailable, updaterStatus, UPDATES_SETTINGS_PATH } from "./updater";
import "./UpdateIndicator.css";

/**
 * "Update 0.2.0" in the top bar once a check — the background one or a manual
 * one in Settings — has found a newer version. Leads to Settings → Updates,
 * where installing is a deliberate click; nothing happens from here alone.
 */
export function UpdateIndicator() {
  const [version, setVersion] = useState<string | null>(null);
  const navigate = useNavigate();

  useEffect(() => {
    updaterStatus()
      .then((s) => {
        if (s.last_check?.available) setVersion(s.last_check.latest_version);
      })
      .catch(() => undefined);
    // No event bridge (tests, a torn-down window) just means no live updates.
    const unlisten = onUpdateAvailable((check) => setVersion(check.latest_version)).catch(() => undefined);
    return () => {
      void unlisten.then((fn) => fn?.());
    };
  }, []);

  if (!version) return null;
  return (
    <button
      type="button"
      className="update-indicator"
      onClick={() => navigate(UPDATES_SETTINGS_PATH)}
      title={`DevsPro ${version} is available`}
    >
      <Icon icon={ArrowUpCircle} size="sm" />
      Update {version}
    </button>
  );
}
