import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../../../app/ConfirmContext";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Checkbox,
  Column,
  DataTable,
  IconButton,
  Page,
  SearchInput,
  Section,
  SegmentedControl,
  Select,
  Stack,
  StatusPill,
  Toolbar,
  statusOf,
  useResource,
} from "../../../ui";
import { Download, ExternalLink, Package, RefreshCw, SearchX } from "../../../ui/icons";
import { AppStatus, InstallOutcome, installBatch, listApps } from "../api";

type StatusFilter = "all" | "installed" | "not_installed";

const STATUS_FILTERS = [
  { value: "all" as StatusFilter, label: "All" },
  { value: "installed" as StatusFilter, label: "Installed" },
  { value: "not_installed" as StatusFilter, label: "Available" },
];

export default function AppsPage() {
  const confirmAction = useConfirm();
  const module = useModule("apps");
  const apps = useResource(listApps);

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [outcomes, setOutcomes] = useState<Record<string, InstallOutcome>>({});
  const [error, setError] = useState<string | null>(null);
  const [activeCategory, setActiveCategory] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [query, setQuery] = useState("");
  const [versions, setVersions] = useState<Record<string, string>>({});

  const rows = apps.data ?? [];

  function versionFor(a: AppStatus): string {
    return versions[a.id] ?? a.available_versions[0] ?? "";
  }

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  async function handleInstall() {
    if (selected.size === 0) return;
    const names = rows.filter((a) => selected.has(a.id)).map((a) => a.name);

    const ok = await confirmAction(
      `${names.join(", ")} will be installed.`,
      {
        title: `Install ${names.length} tool${names.length === 1 ? "" : "s"}?`,
        confirmLabel: "Install",
      },
    );
    if (!ok) return;

    setBusy(true);
    setError(null);
    try {
      const selections = Array.from(selected).map((id) => ({
        id,
        version: versions[id] ?? null,
      }));
      const results = await installBatch(selections);
      setOutcomes(Object.fromEntries(results.map((r) => [r.id, r])));
      setSelected(new Set());
      await apps.refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const categories = Array.from(new Set(rows.map((a) => a.category)));
  const categoryOptions = [
    { value: "all", label: "All" },
    ...categories.map((c) => ({ value: c, label: c })),
  ];

  const q = query.trim().toLowerCase();
  const visible = rows.filter((a) => {
    if (activeCategory !== "all" && a.category !== activeCategory) return false;
    if (statusFilter === "installed" && !a.installed) return false;
    if (statusFilter === "not_installed" && a.installed) return false;
    if (q && !a.name.toLowerCase().includes(q) && !a.description.toLowerCase().includes(q))
      return false;
    return true;
  });

  const selectableIds = visible.filter((a) => !a.installed).map((a) => a.id);
  const allSelected =
    selectableIds.length > 0 && selectableIds.every((id) => selected.has(id));
  const someSelected = selectableIds.some((id) => selected.has(id));

  function toggleAllVisible() {
    setSelected((prev) => {
      const next = new Set(prev);
      if (allSelected) selectableIds.forEach((id) => next.delete(id));
      else selectableIds.forEach((id) => next.add(id));
      return next;
    });
  }

  const outcomeList = Object.values(outcomes);
  const failedCount = outcomeList.filter((o) => !o.success).length;

  const columns: Column<AppStatus>[] = [
    {
      key: "select",
      header: (
        <Checkbox
          checked={allSelected}
          indeterminate={someSelected}
          onChange={toggleAllVisible}
          label="Select all installable tools"
          hideLabel
          disabled={selectableIds.length === 0}
        />
      ),
      priority: 1,
      width: "1px",
      render: (a) => (
        <Checkbox
          checked={selected.has(a.id)}
          onChange={() => toggle(a.id)}
          label={`Select ${a.name}`}
          hideLabel
          disabled={a.installed}
        />
      ),
    },
    {
      key: "name",
      header: "Name",
      priority: 1,
      wrap: true,
      sortValue: (a) => a.name,
      render: (a) => {
        const outcome = outcomes[a.id];
        return (
          <div className="dt-cell-stack">
            <span className="dt-cell-inline">
              <span className="dt-strong">{a.name}</span>
              {/* Method was its own column; as a badge here it costs no width
                  and still answers "how does this get installed?". */}
              <Badge tone="neutral" mono>
                {a.method}
              </Badge>
            </span>
            {/* A dedicated description column is what made this table
                unusable when narrow — as a sub-line it wraps in place. */}
            <span className="dt-cell-sub">{a.description}</span>
            {outcome && (
              <span
                className={
                  outcome.success ? "dt-cell-note-ok" : "dt-cell-note-bad"
                }
              >
                {outcome.message}
              </span>
            )}
          </div>
        );
      },
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      sortValue: (a) => (a.installed ? 1 : 0),
      render: (a) => (
        <StatusPill
          variant={a.installed ? "ok" : "neutral"}
          label={a.installed ? "installed" : "available"}
        />
      ),
    },
    {
      key: "version",
      header: "Version",
      priority: 2,
      minWidth: "8rem",
      render: (a) =>
        a.available_versions.length > 0 ? (
          <Select
            size="sm"
            value={versionFor(a)}
            onChange={(v) => setVersions((prev) => ({ ...prev, [a.id]: v }))}
            options={a.available_versions.map((v) => ({ value: v, label: v }))}
            aria-label={`Version for ${a.name}`}
          />
        ) : (
          <span className="u-muted">default</span>
        ),
    },
    {
      key: "category",
      header: "Category",
      priority: 3,
      sortValue: (a) => a.category,
      render: (a) => a.category,
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (a) => (
        <ActionGroup>
          {a.homepage && (
            <IconButton
              icon={ExternalLink}
              label={`Learn more about ${a.name}`}
              onClick={() => void openUrl(a.homepage!)}
            />
          )}
        </ActionGroup>
      ),
    },
  ];

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(apps)}
      description="apt-based tools you select are installed together in one admin-password prompt. Everything else installs for your own account only, with no elevation — nvm into ~/.nvm, npm tools via your own npm, and standalone binaries like cloudflared into ~/.local/bin."
      actions={
        <>
          <Button
            variant="secondary"
            icon={RefreshCw}
            loading={apps.refreshing}
            onClick={apps.refresh}
          >
            Refresh
          </Button>
          <Button
            variant="primary"
            icon={Download}
            loading={busy}
            disabled={selected.size === 0}
            onClick={handleInstall}
          >
            Install selected ({selected.size})
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {outcomeList.length > 0 && (
          <Callout
            tone={failedCount > 0 ? "warning" : "success"}
            onDismiss={() => setOutcomes({})}
          >
            {outcomeList.length - failedCount} installed
            {failedCount > 0 && `, ${failedCount} failed`}. Per-tool detail is shown on
            each row.
          </Callout>
        )}
        {error && (
          <Callout tone="danger" onDismiss={() => setError(null)}>
            {error}
          </Callout>
        )}

        <Section
          title="Dev tools"
          padding="none"
          toolbar={
            <Toolbar
              end={
                <SegmentedControl
                  ariaLabel="Filter by install status"
                  value={statusFilter}
                  onChange={setStatusFilter}
                  options={STATUS_FILTERS}
                />
              }
            >
              <SearchInput
                value={query}
                onChange={setQuery}
                placeholder="Search by name or description"
              />
              {/* Categories were <Tabs>, which implies switching views. They
                  narrow one list, so they're a filter. */}
              <SegmentedControl
                ariaLabel="Filter by category"
                value={activeCategory}
                onChange={setActiveCategory}
                options={categoryOptions}
              />
            </Toolbar>
          }
        >
          <DataTable
            columns={columns}
            rows={visible}
            rowKey={(a) => a.id}
            loading={apps.loading}
            defaultSort={{ key: "name", dir: "asc" }}
            minWidth={560}
            caption="Developer tools available to install"
            empty={{
              icon: rows.length === 0 ? Package : SearchX,
              title: rows.length === 0 ? "No tools available" : "No matches",
              description: "No tools match the current search and filters.",
              action:
                rows.length > 0 ? (
                  <Button
                    variant="ghost"
                    onClick={() => {
                      setQuery("");
                      setActiveCategory("all");
                      setStatusFilter("all");
                    }}
                  >
                    Clear filters
                  </Button>
                ) : undefined,
            }}
          />
        </Section>
      </Stack>
    </Page>
  );
}
