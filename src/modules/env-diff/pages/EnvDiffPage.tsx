import { useMemo, useRef, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
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
import {
  ArrowLeft,
  ArrowRight,
  Bookmark,
  CircleCheck,
  FileDiff,
  Save,
  Trash2,
} from "../../../ui/icons";
import { readEnvFile } from "../api";
import EnvSource, { messageOf, Notice } from "../components/EnvSource";
import { SaveSessionModal, SessionsModal, SideSummary } from "../components/SessionModals";
import type { CodeEditorHandle, ScrollPos } from "../../../ui/CodeEditor";
import {
  applyChanges,
  arrangeLike,
  countStatuses,
  DiffFilter,
  DiffRow,
  DiffStatus,
  diffEnv,
  EnvEntry,
  EnvSide,
  emptySide,
  isDirty,
  parseEnv,
  removeKeys,
  shortName,
  toChange,
} from "../envFile";
import {
  EnvSession,
  PastedText,
  readPasted,
  SideLabels,
  SideName,
  sideName,
  sideTitle,
  suggestName,
  useEnvSessions,
} from "../sessions";

/** A row's status as its badge says it, each side called what the user calls it. */
function statusText(row: DiffRow, labels: SideLabels): string {
  switch (row.status) {
    case "same":
      return "Same";
    case "changed":
      return "Different value";
    case "commented":
      return `Commented out in ${sideName(labels, row.left?.commented ? "left" : "right")}`;
    case "only-left":
      return `Only in ${sideName(labels, "left")}`;
    case "only-right":
      return `Only in ${sideName(labels, "right")}`;
  }
}

const STATUS_TONE: Record<DiffStatus, BadgeTone> = {
  same: "neutral",
  changed: "warning",
  commented: "info",
  "only-left": "info",
  "only-right": "info",
};

// Problems first when sorted by status.
const STATUS_RANK: Record<DiffStatus, number> = {
  changed: 0,
  commented: 1,
  "only-left": 2,
  "only-right": 3,
  same: 4,
};

/** What copying `row` over to the `to` side will do, as a button label. */
function copyLabel(row: DiffRow, to: SideName, labels: SideLabels): string {
  const fromSide = to === "right" ? "left" : "right";
  const from = row[fromSide];
  const toName = sideName(labels, to);
  if (!row[to]) return `Add to ${toName}`;
  if (row.status === "commented") {
    if (from?.commented) return `Comment out on ${toName}`;
    if (from?.value === row[to]?.value) return `Uncomment on ${toName}`;
  }
  return `Use ${sideName(labels, fromSide)} value on ${toName}`;
}

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
  const [filter, setFilter] = useState<DiffFilter>("diff");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  // On by default: the usual job is reading two copies of the same file side by
  // side, where the line numbers line up.
  const [syncScroll, setSyncScroll] = useState(true);
  // What the user calls each side, e.g. dev and live. Saved with a session.
  const [labels, setLabels] = useState<SideLabels>({});
  const title = (side: SideName) => sideTitle(labels, side);
  const named = (side: SideName) => sideName(labels, side);
  const rename = (side: SideName) => (label: string) =>
    setLabels((current) => {
      const next = { ...current };
      if (label) next[side] = label;
      else delete next[side];
      return next;
    });
  const leftEditor = useRef<CodeEditorHandle>(null);
  const rightEditor = useRef<CodeEditorHandle>(null);
  // Set while both editors jump to a variable's own line: sync scroll would
  // otherwise drag the second one back to wherever the first one landed.
  const jumping = useRef(false);
  const follow = (other: typeof leftEditor) => (pos: ScrollPos) => {
    if (syncScroll && !jumping.current) other.current?.scrollTo(pos);
  };

  /** Selects a variable's line in each editor that has it, focusing the left one first. */
  function showInEditors(row: DiffRow) {
    jumping.current = true;
    window.setTimeout(() => (jumping.current = false), 300);
    if (row.left) leftEditor.current?.goToLine(row.left.line, { focus: true });
    if (row.right) rightEditor.current?.goToLine(row.right.line, { focus: !row.left });
  }
  const confirmAction = useConfirm();
  const sessions = useEnvSessions();
  const [dialog, setDialog] = useState<"save" | "sessions" | null>(null);
  // Bumped when a session opens, so both sides remount and their path boxes
  // show that session's files rather than whatever was typed there before.
  const [generation, setGeneration] = useState(0);
  // A session's files that could not be read, left in the path box to fix.
  const [unread, setUnread] = useState<{ left?: string; right?: string }>({});
  // The session last opened or saved, so saving again offers to update it.
  const [activeName, setActiveName] = useState<string | null>(null);

  const parsedLeft = useMemo(() => parseEnv(left.text), [left.text]);
  const parsedRight = useMemo(() => parseEnv(right.text), [right.text]);
  const rows = useMemo(
    () => diffEnv(parsedLeft, parsedRight, { keysOnly }),
    [parsedLeft, parsedRight, keysOnly],
  );
  const counts = countStatuses(rows);
  const differences =
    counts.changed + counts.commented + counts["only-left"] + counts["only-right"];

  const q = query.trim().toLowerCase();
  const visible = rows.filter((row) => {
    if (filter === "diff" ? row.status === "same" : filter !== "all" && row.status !== filter) {
      return false;
    }
    // A rename's partner matches too, so searching either name shows the pair.
    return !q || [row.key, row.renamed].some((key) => key?.toLowerCase().includes(q));
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
      text: `${changes.length === 1 ? changes[0].key : `${changes.length} variables`} copied to the ${named(to)} file.`,
    });
  }

  // For a variable one file has and the other doesn't need: gone from that file.
  function removeFrom(row: DiffRow, side: "left" | "right") {
    (side === "right" ? setRight : setLeft)((current) => ({
      ...current,
      text: removeKeys(current.text, [row.key]),
    }));
    setNotice({ tone: "success", text: `${row.key} removed from the ${named(side)} file.` });
  }

  const leftName = shortName(left.path, right.path);
  const rightName = shortName(right.path, left.path);
  const missingFromRight = rows.filter((r) => r.status === "only-left");
  const missingFromLeft = rows.filter((r) => r.status === "only-right");

  const filterOptions: { value: DiffFilter; label: string; count: number }[] = [
    { value: "all", label: "All", count: rows.length },
    { value: "diff", label: "Differences", count: differences },
    { value: "only-left", label: `Only in ${named("left")}`, count: counts["only-left"] },
    { value: "only-right", label: `Only in ${named("right")}`, count: counts["only-right"] },
    ...(keysOnly ? [] : [{ value: "changed" as const, label: "Different value", count: counts.changed }]),
    { value: "commented", label: "Commented", count: counts.commented },
    { value: "same", label: "Same", count: counts.same },
  ];

  // Each column's share of the table, in sixths. Keys-only drops the two value
  // columns, and the rest divide the whole width in the same proportions.
  const shares = keysOnly ? 3 : 6;
  const share = (sixths: number) => `${((sixths / shares) * 100).toFixed(2)}%`;

  const columns: Column<DiffRow>[] = [
    {
      key: "key",
      header: "Variable",
      priority: 1,
      width: share(2),
      mono: true,
      sortValue: (r) => r.key,
      render: (r) => {
        const name = (
          <Button variant="link" title="Show in the editors" onClick={() => showInEditors(r)}>
            {r.key}
          </Button>
        );
        // Off everywhere it appears; where it is off on one side only, the
        // status says which.
        return r.status !== "commented" && [r.left, r.right].every((e) => !e || e.commented) ? (
          <Stack direction="row" gap={2} wrap align="center">
            {name}
            <Badge>Commented out</Badge>
          </Stack>
        ) : (
          name
        );
      },
    },
    {
      key: "status",
      header: "Status",
      priority: 1,
      width: share(0.5),
      sortValue: (r) => STATUS_RANK[r.status],
      render: (r) => (
        <Stack direction="row" gap={2} wrap align="center">
          <Badge tone={STATUS_TONE[r.status]}>{statusText(r, labels)}</Badge>
          {r.renamed && (
            <Badge tone="accent">
              {r.status === "only-left" ? "Renamed to" : "Renamed from"} {r.renamed}?
            </Badge>
          )}
        </Stack>
      ),
    },
    ...(keysOnly
      ? []
      : [
          {
            key: "left",
            header: leftName ? `${title("left")} · ${leftName}` : title("left"),
            headerLabel: `${title("left")} value`,
            priority: 2 as const,
            width: share(1.5),
            render: (r: DiffRow) => <ValueCell entry={r.left} reveal={reveal} />,
          },
          {
            key: "right",
            header: rightName ? `${title("right")} · ${rightName}` : title("right"),
            headerLabel: `${title("right")} value`,
            priority: 2 as const,
            width: share(1.5),
            render: (r: DiffRow) => <ValueCell entry={r.right} reveal={reveal} />,
          },
        ]),
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: share(0.5),
      render: (r) => (
        <ActionGroup>
          {r.status !== "same" && r.status !== "only-right" && (
            <IconButton
              icon={ArrowRight}
              label={copyLabel(r, "right", labels)}
              onClick={() => copyAcross([r], "right")}
            />
          )}
          {r.status !== "same" && r.status !== "only-left" && (
            <IconButton
              icon={ArrowLeft}
              label={copyLabel(r, "left", labels)}
              onClick={() => copyAcross([r], "left")}
            />
          )}
          {(r.status === "only-left" || r.status === "only-right") && (
            <IconButton
              icon={Trash2}
              label={`Remove from ${named(r.status === "only-left" ? "left" : "right")}`}
              onClick={() => removeFrom(r, r.status === "only-left" ? "left" : "right")}
            />
          )}
        </ActionGroup>
      ),
    },
  ];

  function arrange(to: "left" | "right") {
    const from = to === "right" ? "left" : "right";
    const [target, reference] = to === "right" ? [right, left] : [left, right];
    const { text, gaps, extras } = arrangeLike(target.text, reference.text);
    (to === "right" ? setRight : setLeft)((side) => ({ ...side, text }));
    setNotice({
      tone: "success",
      text: [
        `The ${named(to)} file now follows the ${named(from)} file's layout.`,
        gaps === 1 && "1 variable it doesn't have is left as a blank line.",
        gaps > 1 && `${gaps} variables it doesn't have are left as blank lines.`,
        extras === 1 && "Its 1 own variable is at the end.",
        extras > 1 && `Its ${extras} own variables are at the end.`,
      ]
        .filter(Boolean)
        .join(" "),
    });
  }
  const canArrange = parsedLeft.definitions.length > 0 && parsedRight.definitions.length > 0;

  async function openSession(session: EnvSession) {
    setDialog(null);
    // Pasted text counts too: it is on no disk, so opening a session loses it.
    const unsaved = [left, right].some((side) => side.text !== (side.disk ?? ""));
    if (
      unsaved &&
      !(await confirmAction("Edits and pasted text that aren't saved to a file will be lost.", {
        title: `Open "${session.name}"?`,
        confirmLabel: "Discard and open",
        danger: true,
      }))
    ) {
      return;
    }
    const pastedText = readPasted(session.name);
    const read = async (name: SideName): Promise<{ side: EnvSide; error?: string }> => {
      if (session.pasted.includes(name)) {
        const text = pastedText[name];
        // A session restored from a backup arrives without its pasted text.
        return text === undefined
          ? {
              side: emptySide(),
              error: `the ${sideName(session.labels, name)} side's pasted text isn't stored on this machine`,
            }
          : { side: { text, path: null, disk: null } };
      }
      const path = session[name];
      if (!path) return { side: emptySide() };
      try {
        const text = await readEnvFile(path);
        return { side: { text, path, disk: text } };
      } catch (e) {
        return { side: emptySide(), error: messageOf(e) };
      }
    };
    const [l, r] = await Promise.all([read("left"), read("right")]);
    setLeft(l.side);
    setRight(r.side);
    setUnread({
      left: l.error ? (session.left ?? undefined) : undefined,
      right: r.error ? (session.right ?? undefined) : undefined,
    });
    setActiveName(session.name);
    setLabels(session.labels);
    setKeysOnly(session.keysOnly);
    setSyncScroll(session.syncScroll);
    setFilter(session.filter);
    setGeneration((g) => g + 1);
    const errors = [l.error, r.error].filter(Boolean);
    setNotice(
      errors.length === 0
        ? { tone: "success", text: `Opened session "${session.name}".` }
        : {
            tone: "danger",
            text: `Opened session "${session.name}", but ${errors.length === 1 ? "one side is" : "both sides are"} empty: ${errors.join("; ")}`,
          },
    );
  }

  const sides: Record<SideName, EnvSide> = { left, right };
  const isPasted = (side: EnvSide) => side.path === null && side.text.trim() !== "";
  const summary = (name: SideName): SideSummary => ({
    label: title(name),
    path: sides[name].path,
    pasted: isPasted(sides[name]),
    dirty: isDirty(sides[name]),
  });

  function saveSession(name: string) {
    setDialog(null);
    const pasted = (["left", "right"] as const).filter((n) => isPasted(sides[n]));
    const text: PastedText = Object.fromEntries(pasted.map((n) => [n, sides[n].text]));
    sessions
      .save(
        { name, left: left.path, right: right.path, pasted, labels, keysOnly, syncScroll, filter },
        text,
      )
      .then(() => {
        setActiveName(name);
        setNotice({ tone: "success", text: `Saved session "${name}".` });
      })
      .catch((e) => setNotice({ tone: "danger", text: `Couldn't save the session: ${messageOf(e)}` }));
  }

  function deleteSession(session: EnvSession) {
    sessions
      .remove(session.name)
      .catch((e) => setNotice({ tone: "danger", text: `Couldn't delete the session: ${messageOf(e)}` }));
  }

  const currentSession = sessions.list.find(
    (s) => s.name === activeName && s.left === left.path && s.right === right.path,
  );

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
      actions={
        <>
          <Button variant="secondary" icon={Bookmark} onClick={() => setDialog("sessions")}>
            Sessions
          </Button>
          <Button variant="secondary" icon={Save} onClick={() => setDialog("save")}>
            Save session
          </Button>
        </>
      }
    >
      <Stack gap={5}>
        {notice && (
          <Callout tone={notice.tone} onDismiss={() => setNotice(null)}>
            {notice.text}
          </Callout>
        )}

        <Stack direction="row" justify="end" align="center" gap={3} wrap>
          {/* Lines up the same variables on the same line numbers — what makes
              sync scroll useful when the files were written in different orders. */}
          <Button
            variant="ghost"
            size="sm"
            icon={ArrowRight}
            disabled={!canArrange}
            onClick={() => arrange("right")}
          >
            Arrange {named("right")} like {named("left")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            icon={ArrowLeft}
            disabled={!canArrange}
            onClick={() => arrange("left")}
          >
            Arrange {named("left")} like {named("right")}
          </Button>
          <Switch checked={syncScroll} onChange={setSyncScroll} label="Sync scroll" />
        </Stack>

        <FormGrid columns={2}>
          <EnvSource
            key={`left-${generation}`}
            label={title("left")}
            name={named("left")}
            onRename={rename("left")}
            side={left}
            issues={parsedLeft.issues}
            onChange={setLeft}
            onNotice={setNotice}
            editorRef={leftEditor}
            onScroll={follow(rightEditor)}
            initialPath={unread.left}
          />
          <EnvSource
            key={`right-${generation}`}
            label={title("right")}
            name={named("right")}
            onRename={rename("right")}
            side={right}
            issues={parsedRight.issues}
            onChange={setRight}
            onNotice={setNotice}
            editorRef={rightEditor}
            onScroll={follow(leftEditor)}
            initialPath={unread.right}
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
                Add {missingFromRight.length} missing to {named("right")}
              </Button>
              <Button
                variant="secondary"
                icon={ArrowLeft}
                disabled={missingFromLeft.length === 0}
                onClick={() => copyAcross(missingFromLeft, "left")}
              >
                Add {missingFromLeft.length} missing to {named("left")}
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
            caption={`Variables in the ${named("left")} and ${named("right")} env files`}
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

      {dialog === "save" && (
        <SaveSessionModal
          left={summary("left")}
          right={summary("right")}
          initialName={currentSession?.name ?? suggestName(left.path, right.path)}
          existing={sessions.list}
          onSave={saveSession}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog === "sessions" && (
        <SessionsModal
          sessions={sessions.list}
          onOpen={openSession}
          onDelete={deleteSession}
          onClose={() => setDialog(null)}
        />
      )}
    </Page>
  );
}
