import { useEffect, useState } from "react";
import { Button, Callout, Field, FormGrid, Section, Skeleton, Stack, TextInput } from "../../../ui";
import { Settings } from "../../../ui/icons";
import { getSettings, setSettings } from "../api";
import type { CleanerSettings as Settings_ } from "../api";
import { errorText } from "./overview/scans";
import { PathList } from "./overview/PathList";

const MB = 1024 * 1024;

/** Where the scans look, what they skip, and how big or idle counts as worth flagging. */
export default function CleanerSettings() {
  const [saved, setSaved] = useState<Settings_ | null>(null);
  const [draft, setDraft] = useState<Settings_ | null>(null);
  const [minMb, setMinMb] = useState("");
  const [idleDays, setIdleDays] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  function adopt(s: Settings_) {
    setSaved(s);
    setDraft(s);
    setMinMb(String(Math.round(s.large_file_min_bytes / MB)));
    setIdleDays(String(s.project_idle_days));
  }

  useEffect(() => {
    let live = true;
    getSettings().then(
      (s) => live && adopt(s),
      (e) => live && setLoadError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, []);

  if (loadError) return <Callout tone="danger">Could not load cleaner settings: {loadError}</Callout>;
  if (!draft || !saved) return <Skeleton />;

  const mb = Number(minMb);
  const days = Number(idleDays);
  const valid = minMb.trim() !== "" && idleDays.trim() !== "" && mb > 0 && Number.isFinite(mb) && days >= 0 && Number.isInteger(days);

  function edit(patch: Partial<Settings_>) {
    setDraft((d) => (d ? { ...d, ...patch } : d));
    setDone(false);
  }

  async function save() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      adopt(await setSettings({ ...draft, large_file_min_bytes: Math.round(mb * MB), project_idle_days: days }));
      setDone(true);
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(false);
  }

  return (
    <Section title="Cleaner settings" icon={Settings} description="Where scans look, and what they leave alone." headingLevel={3}>
      <Stack gap={4}>
        <PathList label="Scan roots" description="Folders searched for large files and the storage map." title="Add a scan root" paths={draft.scan_roots} onChange={(scan_roots) => edit({ scan_roots })} />
        <PathList label="Project roots" description="Folders holding your projects, searched for build leftovers." title="Add a project root" paths={draft.project_roots} onChange={(project_roots) => edit({ project_roots })} />
        <PathList label="Exclusions" description="Never scanned or cleaned." title="Add an exclusion" paths={draft.exclusions} onChange={(exclusions) => edit({ exclusions })} />
        <FormGrid columns={2}>
          <Field label="Large file threshold (MB)" htmlFor="cleaner-min-mb">
            <TextInput id="cleaner-min-mb" inputMode="numeric" value={minMb} onChange={(e) => { setMinMb(e.target.value); setDone(false); }} />
          </Field>
          <Field label="Project idle days" htmlFor="cleaner-idle-days" hint="A project untouched this long has its leftovers suggested.">
            <TextInput id="cleaner-idle-days" inputMode="numeric" value={idleDays} onChange={(e) => { setIdleDays(e.target.value); setDone(false); }} />
          </Field>
        </FormGrid>
        {error && <Callout tone="danger">{error}</Callout>}
        {done && <Callout tone="success">Settings saved.</Callout>}
        <div>
          <Button variant="primary" loading={busy} disabled={!valid} onClick={save}>
            Save settings
          </Button>
        </div>
      </Stack>
    </Section>
  );
}
