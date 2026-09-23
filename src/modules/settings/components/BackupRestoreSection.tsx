import { useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  Button,
  Callout,
  Checkbox,
  Field,
  Modal,
  ModalFooter,
  Mono,
  Section,
  SegmentedControl,
  Stack,
} from "../../../ui";
import { Download, HardDrive, Upload } from "../../../ui/icons";
import {
  backupExport,
  backupImport,
  backupInspect,
  type BackupImportSummary,
  type BackupInspection,
  type BackupMode,
  type BackupSection,
} from "../api";
import {
  BACKUP_SECTION_LABELS,
  BACKUP_SECTION_ORDER,
  defaultBackupFilename,
  defaultSelection,
  describeCounts,
} from "../backup";

const JSON_FILTER = [{ name: "DevsPro backup", extensions: ["json"] }];

interface PendingImport {
  path: string;
  inspection: BackupInspection;
}

/**
 * Export/import of the app's definitions as one JSON file. The import writes
 * rows and CA files only — it never touches nginx or issues certificates, so the
 * summary always ends with what to re-apply on the Sites tab.
 */
export default function BackupRestoreSection() {
  const confirmAction = useConfirm();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exported, setExported] = useState<{
    path: string;
    inspection: BackupInspection;
  } | null>(null);
  const [pending, setPending] = useState<PendingImport | null>(null);
  const [selected, setSelected] = useState<BackupSection[]>([]);
  const [mode, setMode] = useState<BackupMode>("merge");
  const [summary, setSummary] = useState<BackupImportSummary | null>(null);

  function fail(e: unknown) {
    setError(e instanceof Error ? e.message : String(e));
  }

  async function handleExport() {
    const path = await save({
      title: "Export DevsPro configuration",
      defaultPath: defaultBackupFilename(new Date()),
      filters: JSON_FILTER,
    });
    if (!path) return;
    setBusy(true);
    setError(null);
    setSummary(null);
    try {
      setExported({ path, inspection: await backupExport(path) });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  async function handlePickImport() {
    const selectedPath = await open({
      multiple: false,
      title: "Select a DevsPro backup to import",
      filters: JSON_FILTER,
    });
    if (!selectedPath || Array.isArray(selectedPath)) return;
    setBusy(true);
    setError(null);
    setSummary(null);
    setExported(null);
    try {
      const inspection = await backupInspect(selectedPath);
      setSelected(defaultSelection(inspection.sections));
      setMode("merge");
      setPending({ path: selectedPath, inspection });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  function toggleSection(section: BackupSection, checked: boolean) {
    setSelected((prev) =>
      checked ? [...prev, section] : prev.filter((s) => s !== section),
    );
  }

  async function handleImport() {
    if (!pending) return;
    if (mode === "replace") {
      const ok = await confirmAction(
        `Replace deletes every existing ${selected
          .filter((s) => s !== "ca")
          .map((s) => BACKUP_SECTION_LABELS[s].toLowerCase())
          .join(", ")} row before importing. This cannot be undone.`,
        {
          title: "Replace existing data?",
          confirmLabel: "Replace",
          danger: true,
        },
      );
      if (!ok) return;
    }
    setBusy(true);
    setError(null);
    try {
      setSummary(await backupImport(pending.path, selected, mode));
      setPending(null);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  const countOf = (section: BackupSection) =>
    pending?.inspection.sections.find((s) => s.section === section)?.count ?? 0;

  return (
    <Section
      title="Backup & restore"
      icon={HardDrive}
      description="Move your sites, workers, preferences and root CA to another machine as one JSON file. Importing only restores the definitions; certificates and nginx configs are re-created from the Sites tab afterwards."
    >
      <Stack gap={4}>
        <div>
          <Button
            variant="secondary"
            icon={Download}
            loading={busy}
            onClick={handleExport}
          >
            Export…
          </Button>{" "}
          <Button
            variant="secondary"
            icon={Upload}
            loading={busy}
            onClick={handlePickImport}
          >
            Import…
          </Button>
        </div>
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}
        {exported && (
          <Callout
            tone="success"
            title="Exported"
            onDismiss={() => setExported(null)}
          >
            Wrote {describeCounts(exported.inspection.sections)} to{" "}
            <Mono>{exported.path}</Mono>.
          </Callout>
        )}
        {summary && (
          <Callout
            tone={summary.needs_apply ? "warning" : "success"}
            title={`Imported ${describeCounts(summary.imported)}`}
            onDismiss={() => setSummary(null)}
          >
            <Stack gap={2}>
              <p>{summary.note}</p>
              {summary.ca_backups.length > 0 && (
                <p>
                  Previous CA files kept as:{" "}
                  {summary.ca_backups.map((p) => (
                    <Mono key={p}>{p} </Mono>
                  ))}
                </p>
              )}
            </Stack>
          </Callout>
        )}
      </Stack>

      {pending && (
        <Modal
          title="Import configuration"
          icon={Upload}
          description={
            <>
              Exported {pending.inspection.exported_at} from{" "}
              <Mono>{pending.path}</Mono>
            </>
          }
          onClose={() => setPending(null)}
          footer={
            <ModalFooter>
              <Button variant="secondary" onClick={() => setPending(null)}>
                Cancel
              </Button>
              <Button
                loading={busy}
                disabled={selected.length === 0}
                onClick={handleImport}
              >
                Import
              </Button>
            </ModalFooter>
          }
        >
          <Stack gap={5}>
            <Field
              label="Sections"
              description="Empty sections are unchecked so they cannot wipe local data."
            >
              <Stack gap={2}>
                {BACKUP_SECTION_ORDER.map((section) => (
                  <Checkbox
                    key={section}
                    checked={selected.includes(section)}
                    disabled={countOf(section) === 0}
                    onChange={(checked) => toggleSection(section, checked)}
                    label={
                      section === "ca"
                        ? countOf(section)
                          ? "Root CA (replaces the local one; old files are kept as .bak)"
                          : "Root CA (not in this backup)"
                        : `${BACKUP_SECTION_LABELS[section]} (${countOf(section)})`
                    }
                  />
                ))}
              </Stack>
            </Field>
            <Field
              label="Mode"
              description="Merge updates rows with the same id and keeps the rest. Replace deletes each selected section first."
            >
              <SegmentedControl
                ariaLabel="Import mode"
                size="md"
                value={mode}
                onChange={setMode}
                options={[
                  { value: "merge", label: "Merge" },
                  { value: "replace", label: "Replace" },
                ]}
              />
            </Field>
          </Stack>
        </Modal>
      )}
    </Section>
  );
}
