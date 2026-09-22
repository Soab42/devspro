import { useMemo, useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import {
  ActionGroup,
  Badge,
  BadgeTone,
  Button,
  Callout,
  Column,
  DataTable,
  FormGrid,
  IconButton,
  Mono,
  Page,
  SearchInput,
  Section,
  SegmentedControl,
  Stack,
  Switch,
  Toolbar,
} from "../../../ui";
import { ArrowLeft, ArrowRight, CircleCheck, FileDiff } from "../../../ui/icons";
import EnvSource, { Notice } from "../components/EnvSource";
import {
  applyChanges,
  countStatuses,
  DiffRow,
  DiffStatus,
  diffEnv,
  EnvEntry,
  EnvSide,
  emptySide,
  parseEnv,
  shortName,
  toChange,
} from "../envFile";

type Filter = "all" | "diff" | DiffStatus;

const STATUS_LABEL: Record<DiffStatus, string> = {
  same: "Same",
  changed: "Different value",
  "only-left": "Only in left",
  "only-right": "Only in right",
};

const STATUS_TONE: Record<DiffStatus, BadgeTone> = {
  same: "neutral",
  changed: "warning",
  "only-left": "info",
  "only-right": "info",
};

// Problems first when sorted by status.
const STATUS_RANK: Record<DiffStatus, number> = {
  changed: 0,
  "only-left": 1,
  "only-right": 2,
  same: 3,
};

const MASK = "••••••••";

function ValueCell({ entry, reveal }: { entry: EnvEntry | undefined; reveal: boolean }) {
  if (!entry) return <span className="u-muted">missing</span>;
  if (entry.value === "") return <span className="u-muted">(empty)</span>;
  return (
    <Mono truncate title={reveal ? entry.value : undefined}>
      {reveal ? entry.value : MASK}
    </Mono>
  );
}

export default function EnvDiffPage() {
  const module = useModule("env-diff");
  const [left, setLeft] = useState<EnvSide>(emptySide);
  const [right, setRight] = useState<EnvSide>(emptySide);
  const [keysOnly, setKeysOnly] = useState(false);
  // Off by default: env files are where secrets live, and this page is the kind
  // of thing that ends up on a screen share.
  const [reveal, setReveal] = useState(false);
  const [filter, setFilter] = useState<Filter>("diff");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);

  const parsedLeft = useMemo(() => parseEnv(left.text), [left.text]);
  const parsedRight = useMemo(() => parseEnv(right.text), [right.text]);
  const rows = useMemo(
    () => diffEnv(parsedLeft, parsedRight, { keysOnly }),
    [parsedLeft, parsedRight, keysOnly],
  );
  const counts = countStatuses(rows);
  const differences = counts.changed + counts["only-left"] + counts["only-right"];

  const q = query.trim().toLowerCase();
  const visible = rows.filter((row) => {
    if (filter === "diff" ? row.status === "same" : filter !== "all" && row.status !== filter) {
      return false;
    }
    return !q || row.key.toLowerCase().includes(q);
  });

  function copyAcross(picked: DiffRow[], to: "left" | "right") {
    const from = to === "right" ? "left" : "right";
    const changes = picked.flatMap((row) => (row[from] ? [toChange(row[from])] : []));
    if (changes.length === 0) return;
    (to === "right" ? setRight : setLeft)((side) => ({
      ...side,
      text: applyChanges(side.text, changes),
    }));
    setNotice({
      tone: "success",
      text: `${changes.length === 1 ? changes[0].key : `${changes.length} variables`} copied to the ${to} file.`,
    });
  }

  const leftName = shortName(left.path, right.path);
  const rightName = shortName(right.path, left.path);
  const missingFromRight = rows.filter((r) => r.status === "only-left");
  const missingFromLeft = rows.filter((r) => r.status === "only-right");

  const filterOptions: { value: Filter; label: string; count: number }[] = [
    { value: "all", label: "All", count: rows.length },
    { value: "diff", label: "Differences", count: differences },
    { value: "only-left", label: "Only in left", count: counts["only-left"] },
    { value: "only-right", label: "Only in right", count: counts["only-right"] },
    ...(keysOnly ? [] : [{ value: "changed" as const, label: "Different value", count: counts.changed }]),
    { value: "same", label: "Same", count: counts.same },
  ];

  const columns: Column<DiffRow>[] = [
    {
      key: "key",
      header: "Variable",
      priority: 1,
      mono: true,
      sortValue: (r) => r.key,
      render: (r) => r.key,
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      sortValue: (r) => STATUS_RANK[r.status],
      render: (r) => <Badge tone={STATUS_TONE[r.status]}>{STATUS_LABEL[r.status]}</Badge>,
    },
    ...(keysOnly
      ? []
      : [
          {
            key: "left",
            header: leftName ? `Left · ${leftName}` : "Left",
            headerLabel: "Left value",
            priority: 2 as const,
            render: (r: DiffRow) => <ValueCell entry={r.left} reveal={reveal} />,
          },
          {
            key: "right",
            header: rightName ? `Right · ${rightName}` : "Right",
            headerLabel: "Right value",
            priority: 2 as const,
            render: (r: DiffRow) => <ValueCell entry={r.right} reveal={reveal} />,
          },
        ]),
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => (
        <ActionGroup>
          {(r.status === "only-left" || r.status === "changed") && (
            <IconButton
              icon={ArrowRight}
              label={r.status === "changed" ? "Use left value on right" : "Add to right"}
              onClick={() => copyAcross([r], "right")}
            />
          )}
          {(r.status === "only-right" || r.status === "changed") && (
            <IconButton
              icon={ArrowLeft}
              label={r.status === "changed" ? "Use right value on left" : "Add to left"}
              onClick={() => copyAcross([r], "left")}
            />
          )}
        </ActionGroup>
      ),
    },
  ];

  const nothingYet = !left.text.trim() && !right.text.trim();

  return (
    <Page
      title={module.label}
      icon={module.icon}
      description={
        <>
          Compares two env files by variable, not by line. Where a variable sits, blank lines,
          comments and quoting don&apos;t matter — only which variables exist and what they are
          set to. Copy across any that are missing or different and the file keeps its own
          layout.
        </>
      }
    >
      <Stack gap={5}>
        {notice && (
          <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
            {notice.text}
          </Callout>
        )}

        <FormGrid columns={2}>
          <EnvSource
            label="Left"
            side={left}
            issues={parsedLeft.issues}
            onChange={setLeft}
            onNotice={setNotice}
          />
          <EnvSource
            label="Right"
            side={right}
            issues={parsedRight.issues}
            onChange={setRight}
            onNotice={setNotice}
          />
        </FormGrid>

        <Section
          title="Comparison"
          padding="none"
          isEmpty={nothingYet}
          empty={{
            icon: FileDiff,
            title: "Nothing to compare yet",
            description: "Load or paste an env file into each side above.",
          }}
          actions={
            <>
              <Button
                variant="secondary"
                icon={ArrowRight}
                disabled={missingFromRight.length === 0}
                onClick={() => copyAcross(missingFromRight, "right")}
              >
                Add {missingFromRight.length} missing to right
              </Button>
              <Button
                variant="secondary"
                icon={ArrowLeft}
                disabled={missingFromLeft.length === 0}
                onClick={() => copyAcross(missingFromLeft, "left")}
              >
                Add {missingFromLeft.length} missing to left
              </Button>
            </>
          }
          toolbar={
            <Toolbar
              end={
                <Stack direction="row" gap={5} align="center">
                  <Switch
                    checked={keysOnly}
                    onChange={setKeysOnly}
                    label="Keys only"
                  />
                  <Switch checked={reveal} onChange={setReveal} label="Show values" />
                </Stack>
              }
            >
              <SearchInput
                value={query}
                onChange={setQuery}
                placeholder="Filter by variable name"
                aria-label="Filter by variable name"
              />
              <SegmentedControl
                ariaLabel="Filter by status"
                value={filter}
                onChange={setFilter}
                options={filterOptions}
              />
            </Toolbar>
          }
        >
          <DataTable
            columns={columns}
            rows={visible}
            rowKey={(r) => r.key}
            defaultSort={{ key: "key", dir: "asc" }}
            caption="Variables in the left and right env files"
            empty={
              rows.length > 0 && differences === 0 && filter === "diff" && !q
                ? {
                    icon: CircleCheck,
                    title: "No differences",
                    description: keysOnly
                      ? "Both files define the same variables."
                      : "Both files define the same variables with the same values.",
                  }
                : {
                    icon: FileDiff,
                    title: "No matching variables",
                    description: "Nothing matches the current search and filter.",
                  }
            }
          />
        </Section>
      </Stack>
    </Page>
  );
}
