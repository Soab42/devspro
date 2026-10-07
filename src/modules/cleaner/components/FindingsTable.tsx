import { useMemo } from "react";
import { Badge, BadgeTone, Checkbox, Column, DataTable, EmptyState, Mono, Stack } from "../../../ui";
import type { EmptyStateProps } from "../../../ui";
import type { Finding, Risk } from "../api";
import { formatBytes, groupFindings, RISK_LABEL } from "../cleaner";

const RISK_TONE: Record<Risk, BadgeTone> = { safe: "success", review: "warning", danger: "danger" };

export interface FindingsTableProps {
  findings: Finding[];
  /** Selected finding ids. Owned by the parent. */
  selected: Set<string>;
  onSelectionChange: (selected: Set<string>) => void;
  loading?: boolean;
  empty?: EmptyStateProps;
}

/** Findings under group headings, with a checkbox each. Blocked ones can't be ticked. */
export function FindingsTable({ findings, selected, onSelectionChange, loading, empty }: FindingsTableProps) {
  const groups = useMemo(() => groupFindings(findings), [findings]);

  function toggle(ids: string[], on: boolean) {
    const next = new Set(selected);
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    onSelectionChange(next);
  }

  const columns: Column<Finding>[] = [
    {
      key: "select",
      header: "",
      headerLabel: "Select",
      width: "2.5rem",
      render: (f) => (
        <Checkbox
          hideLabel
          label={`Select ${f.label}`}
          checked={selected.has(f.id)}
          disabled={Boolean(f.blocked_by)}
          onChange={(on) => toggle([f.id], on)}
        />
      ),
    },
    {
      key: "label",
      header: "Item",
      wrap: true,
      render: (f) => (
        <Stack gap={1}>
          <span>{f.label}</span>
          <span className="u-muted">{f.reason}</span>
          {f.blocked_by && <Badge tone="warning">Blocked: {f.blocked_by}</Badge>}
        </Stack>
      ),
    },
    {
      key: "risk",
      header: "Risk",
      priority: 2,
      render: (f) => <Badge tone={RISK_TONE[f.risk]}>{RISK_LABEL[f.risk]}</Badge>,
    },
    {
      key: "bytes",
      header: "Size",
      align: "end",
      render: (f) => <Mono>{formatBytes(f.bytes)}</Mono>,
    },
  ];

  if (!loading && findings.length === 0) return <EmptyState {...(empty ?? { title: "Nothing found" })} />;

  return (
    <Stack gap={4}>
      {(loading ? [] : groups).map((g) => {
        const selectable = g.findings.filter((f) => !f.blocked_by).map((f) => f.id);
        const ticked = selectable.filter((id) => selected.has(id)).length;
        return (
          <section key={g.group} aria-label={g.group}>
            <Stack gap={2}>
              <Checkbox
                label={`${g.group} · ${formatBytes(g.bytes)}`}
                checked={selectable.length > 0 && ticked === selectable.length}
                indeterminate={ticked > 0 && ticked < selectable.length}
                disabled={selectable.length === 0}
                onChange={(on) => toggle(selectable, on)}
              />
              <DataTable
                caption={g.group}
                columns={columns}
                rows={g.findings}
                rowKey={(f) => f.id}
                density="compact"
              />
            </Stack>
          </section>
        );
      })}
      {loading && <DataTable columns={columns} rows={[]} rowKey={(f) => f.id} loading />}
    </Stack>
  );
}

export default FindingsTable;
