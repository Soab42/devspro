import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Card, Checkbox, DataTable, Meter, Mono, Select, Stack, Toolbar } from "../../../ui";
import type { Column } from "../../../ui";
import { ChevronDown, ChevronRight, Eraser, Files, X } from "../../../ui/icons";
import { cancelScan, getSettings, onScanProgress, removeFiles, scanTree, treeEstimate } from "../api";
import type { CleanOutcome, ExtStat, Finding, ScanProgress, StorageTree, TreeNode } from "../api";
import { formatBytes, newScanId } from "../cleaner";
import ReviewCleanModal from "./ReviewCleanModal";
import ScanStamp from "./ScanStamp";
import { getSlot, setSlot, useTabRestore, useTabSlot } from "../tabState";
import { colorOfKind, FOLDER_COLOR, KIND_LABELS, kindOfName, layoutTree, type Cell } from "./storage-map/treemap";

interface TreeRow {
  node: TreeNode;
  depth: number;
  key: string;
}

const TAB = "storage-map";
const CHOOSE = "__choose__";
const WIDTH = 1000;
const HEIGHT = 520;
const BOUNDS = { x: 0, y: 0, w: WIDTH, h: HEIGHT };

/** The chain of folders from `root` down to the node at `target`, or null. */
function pathTo(root: TreeNode, target: string): TreeNode[] | null {
  const chain = [root];
  let node = root;
  while (node.path !== target) {
    const next = node.children.find((c) => c.path !== "" && (c.path === target || target.startsWith(c.path + "/")));
    if (!next) return null;
    chain.push(next);
    node = next;
  }
  return chain;
}

/** The tree without the node at `path`, ancestors' sizes reduced to match. */
function prune(node: TreeNode, path: string): TreeNode {
  const hit = node.children.find((c) => c.path === path);
  if (hit) {
    return {
      ...node,
      bytes: node.bytes - hit.bytes,
      files: node.files - hit.files,
      children: node.children.filter((c) => c !== hit),
    };
  }
  const via = node.children.find((c) => c.path !== "" && path.startsWith(c.path + "/"));
  if (!via) return node;
  const child = prune(via, path);
  return {
    ...node,
    bytes: node.bytes - (via.bytes - child.bytes),
    files: node.files - (via.files - child.files),
    children: node.children.map((c) => (c === via ? child : c)),
  };
}

function parentDir(path: string): string {
  const i = path.lastIndexOf("/");
  return i > 0 ? path.slice(0, i) : "/";
}

function colorOf(node: TreeNode): string {
  if (node.is_dir) return FOLDER_COLOR;
  return node.path === "" ? colorOfKind("other") : colorOfKind(kindOfName(node.name));
}

function fit(text: string, width: number): string {
  const max = Math.floor(width / 6.5);
  return text.length <= max ? text : max > 1 ? `${text.slice(0, max - 1)}…` : "";
}

const percent = (part: number, whole: number) => (whole > 0 ? (part / whole) * 100 : 0);
const formatPercent = (p: number) => (p > 0 && p < 0.1 ? "<0.1%" : `${p.toFixed(p >= 10 ? 0 : 1)}%`);

export default function StorageMapTab() {
  const navigate = useNavigate();
  const ready = useTabRestore(TAB);
  const [roots, setRoots] = useState<string[]>([]);
  const [root, setRoot] = useTabSlot(TAB, "root", "");
  const [tree, setTree] = useTabSlot<StorageTree | null>(TAB, "tree", null);
  const [scanning] = useTabSlot(TAB, "scanning", false);
  const [progress] = useTabSlot<ScanProgress | null>(TAB, "progress", null);
  const [estimate] = useTabSlot<number | null>(TAB, "estimate", null);
  const [error, setError] = useTabSlot<string | null>(TAB, "error", null);
  const [scannedAt] = useTabSlot<number | null>(TAB, "scannedAt", null);
  const [trimmed] = useTabSlot(TAB, "trimmed", false);
  const [zoomPath, setZoomPath] = useTabSlot(TAB, "zoomPath", "");
  const [selected, setSelected] = useTabSlot<string | null>(TAB, "selected", null);
  const [expandedList, setExpandedList] = useTabSlot<string[]>(TAB, "expanded", []);
  const expanded = useMemo(() => new Set(expandedList), [expandedList]);
  const [hover, setHover] = useState<TreeNode | null>(null);
  const [reviewing, setReviewing] = useState<TreeNode | null>(null);
  const [permanent, setPermanent] = useState(false);

  useEffect(() => {
    if (!ready) return;
    let live = true;
    getSettings()
      .then((s) => {
        if (!live) return;
        setRoots(s.scan_roots);
        setRoot((r) => r || s.scan_roots[0] || "");
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [ready, setRoot, setError]);

  const options = useMemo(() => {
    const paths = root && !roots.includes(root) ? [...roots, root] : roots;
    return [...paths.map((p) => ({ value: p, label: p })), { value: CHOOSE, label: "Choose folder…" }];
  }, [roots, root]);

  async function choose(value: string) {
    if (value !== CHOOSE) return setRoot(value);
    const picked = await open({ directory: true, multiple: false, title: "Choose a folder to map" });
    if (typeof picked === "string") setRoot(picked);
  }

  // The scan writes to the tab store, so it finishes into it even if this tab is left.
  async function scan() {
    const id = newScanId();
    setSlot(TAB, "scanId", id, null as string | null);
    setSlot(TAB, "scanning", true, false);
    setSlot(TAB, "trimmed", false, false);
    setSlot(TAB, "error", null as string | null, null);
    setSlot(TAB, "progress", null as ScanProgress | null, null);
    // The bar's total: this folder's size from its last full scan, else the
    // space in use when it is a whole drive.
    const known = getSlot<Record<string, number>>(TAB, "sizes", {})[root] ?? null;
    setSlot(TAB, "estimate", known, null);
    if (known === null) {
      void treeEstimate(root)
        .then((e) => {
          if (e && getSlot<string | null>(TAB, "scanId", null) === id) setSlot(TAB, "estimate", e as number | null, null);
        })
        .catch(() => {});
    }
    const off = await onScanProgress((p) => {
      if (p.scan_id === id) setSlot(TAB, "progress", p as ScanProgress | null, null);
    }).catch(() => () => {});
    try {
      const result = await scanTree(id, root);
      setSlot(TAB, "tree", result as StorageTree | null, null);
      setSlot(TAB, "zoomPath", result.root.path, "");
      setSlot(TAB, "selected", null as string | null, null);
      setSlot(TAB, "expanded", [] as string[], []);
      setSlot(TAB, "scannedAt", Date.now() as number | null, null);
      if (!result.truncated) {
        setSlot(TAB, "sizes", { ...getSlot<Record<string, number>>(TAB, "sizes", {}), [root]: result.total_bytes }, {});
      }
      setHover(null);
    } catch (e) {
      setSlot(TAB, "error", (e instanceof Error ? e.message : String(e)) as string | null, null);
    } finally {
      off();
      setSlot(TAB, "scanId", null as string | null, null);
      setSlot(TAB, "scanning", false, false);
    }
  }

  function cancel() {
    const id = getSlot<string | null>(TAB, "scanId", null);
    if (id) void cancelScan(id).catch(() => {});
  }

  const chain = useMemo(() => (tree ? (pathTo(tree.root, zoomPath) ?? [tree.root]) : []), [tree, zoomPath]);
  const current = chain[chain.length - 1] ?? null;
  const cells = useMemo(() => (current ? layoutTree(current, BOUNDS) : []), [current]);
  const selectedNode = useMemo(() => {
    if (!tree || !selected) return null;
    const c = pathTo(tree.root, selected);
    return c ? c[c.length - 1] : null;
  }, [tree, selected]);

  /** Selects a node and opens the tree rows above it, so its row is visible. */
  const select = useCallback(
    (path: string) => {
      setSelected(path);
      const above = tree ? pathTo(tree.root, path) : null;
      if (above && above.length > 1) {
        setExpandedList((prev) => [...new Set([...prev, ...above.slice(0, -1).map((n) => n.path)])]);
      }
    },
    [tree, setSelected, setExpandedList],
  );

  const zoomTo = useCallback(
    (node: TreeNode) => {
      setZoomPath(node.path);
      setSelected(node.path);
    },
    [setZoomPath, setSelected],
  );

  const click = (node: TreeNode) => {
    if (node.path === "") return;
    if (node.is_dir) zoomTo(node);
    else select(node.path);
  };

  const toggle = (path: string) =>
    setExpandedList((prev) => {
      const next = new Set(prev);
      if (!next.delete(path)) next.add(path);
      return [...next];
    });

  /** The zoomed folder's contents as visible tree rows: expanded folders show theirs below them. */
  const rows = useMemo(() => {
    const out: TreeRow[] = [];
    const walk = (node: TreeNode, depth: number, parentKey: string) => {
      node.children.forEach((c, i) => {
        const key = c.path || `${parentKey}#folded-${i}`;
        out.push({ node: c, depth, key });
        if (c.is_dir && expanded.has(c.path)) walk(c, depth + 1, key);
      });
    };
    if (current) walk(current, 0, current.path);
    return out;
  }, [current, expanded]);

  const finding = (node: TreeNode): Finding => ({
    id: node.path,
    tab: "storage-map",
    group: "Storage map",
    label: node.name,
    reason: node.is_dir ? "Folder chosen in the storage map" : "File chosen in the storage map",
    method: permanent ? "Delete permanently" : "Trash",
    paths: [node.path],
    bytes: node.bytes,
    risk: "review",
    suggested: false,
    blocked_by: null,
    needs_confirm_text: null,
  });

  async function remove(ids: string[]): Promise<CleanOutcome[]> {
    const outcomes = await removeFiles(ids, permanent);
    const paths = outcomes.filter((o) => o.ok).map((o) => o.id);
    setTree((t) => {
      if (!t) return t;
      let next = t.root;
      let removed = 0;
      for (const p of paths) {
        const before = next.bytes;
        next = prune(next, p);
        removed += before - next.bytes;
      }
      return { ...t, root: next, total_bytes: t.total_bytes - removed };
    });
    if (paths.length > 0) {
      setSlot(TAB, "trimmed", true, false);
      setSelected(null);
      setHover(null);
      setZoomPath((z) => (paths.some((p) => z === p || z.startsWith(p + "/")) ? (tree?.root.path ?? z) : z));
    }
    return outcomes;
  }

  const total = tree?.total_bytes ?? 0;

  const treeColumns: Column<TreeRow>[] = [
    {
      key: "name",
      header: "Name",
      priority: 1,
      render: ({ node: n, depth }) => (
        <span>
          {"\u00a0\u00a0\u00a0\u00a0".repeat(depth)}
          {n.is_dir && n.children.length > 0 ? (
            <Button
              size="sm"
              variant="ghost"
              icon={expanded.has(n.path) ? ChevronDown : ChevronRight}
              aria-label={`${expanded.has(n.path) ? "Collapse" : "Expand"} ${n.name}`}
              aria-expanded={expanded.has(n.path)}
              onClick={() => toggle(n.path)}
            />
          ) : (
            "\u00a0\u00a0\u00a0\u00a0"
          )}
          <Button size="sm" variant="ghost" aria-pressed={n.path !== "" && n.path === selected} onClick={() => n.path !== "" && select(n.path)}>
            {n.path !== "" && n.path === selected ? <strong>{n.name}</strong> : n.name}
          </Button>
        </span>
      ),
    },
    { key: "size", header: "Size", align: "end", mono: true, priority: 1, render: ({ node: n }) => formatBytes(n.bytes) },
    {
      key: "share",
      header: "Share",
      priority: 2,
      minWidth: "140px",
      render: ({ node: n }) => {
        const p = percent(n.bytes, current?.bytes ?? 0);
        return <Meter percent={p} valueLabel={formatPercent(p)} />;
      },
    },
  ];

  const extColumns: Column<ExtStat>[] = [
    {
      key: "colour",
      header: "Colour",
      width: "64px",
      priority: 1,
      render: (e) => (
        <svg width="14" height="14" aria-hidden="true">
          <rect width="14" height="14" rx="3" fill={colorOfKind(e.kind)} />
        </svg>
      ),
    },
    { key: "ext", header: "Extension", priority: 1, mono: true, render: (e) => e.ext, sortValue: (e) => e.ext },
    { key: "kind", header: "Kind", priority: 2, render: (e) => KIND_LABELS[e.kind] ?? e.kind, sortValue: (e) => e.kind },
    { key: "bytes", header: "Size", align: "end", mono: true, priority: 1, render: (e) => formatBytes(e.bytes), sortValue: (e) => e.bytes },
    { key: "files", header: "Files", align: "end", mono: true, priority: 2, render: (e) => e.files.toLocaleString(), sortValue: (e) => e.files },
  ];

  const canAct = selectedNode !== null && selectedNode.path !== "";
  const isRoot = selectedNode !== null && tree !== null && selectedNode.path === tree.root.path;

  return (
    <Stack gap={4}>
      <Toolbar
        end={
          scanning ? (
            <Button icon={X} onClick={cancel}>
              Cancel
            </Button>
          ) : (
            <Button variant="primary" disabled={!root} onClick={() => void scan()}>
              Scan
            </Button>
          )
        }
      >
        <Select aria-label="Folder to map" value={root} onChange={(v) => void choose(v)} options={options} placeholder="Choose a folder" disabled={scanning} />
      </Toolbar>

      {scanning && <ScanStatus progress={progress} estimate={estimate} />}
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      {tree && !scanning && <ScanStamp at={scannedAt} trimmed={trimmed} />}
      {tree?.truncated && (
        <Callout tone="warning" title="Partial map">
          The scan was cancelled or hit its limit before it finished, so sizes here are a lower bound.
        </Callout>
      )}

      {tree && current && (
        <>
          <Card
            title={
              <nav aria-label="Breadcrumb">
                {chain.map((n, i) => (
                  <span key={n.path}>
                    {i > 0 && " / "}
                    {i === chain.length - 1 ? (
                      <strong>{n.name}</strong>
                    ) : (
                      <Button size="sm" variant="ghost" onClick={() => setZoomPath(n.path)}>
                        {n.name}
                      </Button>
                    )}
                  </span>
                ))}
              </nav>
            }
            description={`${formatBytes(current.bytes)} in ${current.files.toLocaleString()} files; ${formatBytes(total)} mapped in all`}
            actions={
              <>
                <Button size="sm" icon={Files} disabled={!canAct} onClick={() => selectedNode && navigate(`/files?path=${encodeURIComponent(selectedNode.is_dir ? selectedNode.path : parentDir(selectedNode.path))}`)}>
                  Reveal in Files
                </Button>
                <Button size="sm" variant="danger" icon={Eraser} disabled={!canAct || isRoot} onClick={() => selectedNode && setReviewing(selectedNode)}>
                  Remove
                </Button>
              </>
            }
          >
            <Stack gap={3}>
              <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} width="100%" role="group" aria-label="Treemap" onMouseLeave={() => setHover(null)}>
                {cells.map((c, i) => (
                  <CellShape key={c.node.path || `folded-${i}`} cell={c} total={total} selected={c.node.path !== "" && c.node.path === selected} onClick={click} onHover={setHover} />
                ))}
              </svg>
              <div aria-live="polite">
                {hover ? (
                  <span>
                    <Mono>{hover.path || `${hover.name} (not listed individually)`}</Mono> {formatBytes(hover.bytes)}, {formatPercent(percent(hover.bytes, total))} of the map
                  </span>
                ) : selectedNode ? (
                  <span>
                    Selected <Mono>{selectedNode.path}</Mono> {formatBytes(selectedNode.bytes)}, {formatPercent(percent(selectedNode.bytes, total))} of the map
                  </span>
                ) : (
                  <span>Hover a rectangle for its size. Click a folder to zoom in.</span>
                )}
              </div>
              <Checkbox label="Delete permanently instead of moving to Trash" checked={permanent} onChange={setPermanent} />
            </Stack>
          </Card>

          <Card title="Folder contents" description="Largest first" padding="none">
            <DataTable
              columns={treeColumns}
              rows={rows}
              rowKey={(r) => r.key}
              caption="Contents of the current folder, largest first"
              density="compact"
              onRowDoubleClick={(r) => click(r.node)}
              empty={{ title: "Nothing here", description: "This folder has no sized entries." }}
            />
          </Card>

          <Card title="File types" description="Every file in the scan, by extension" padding="none">
            <DataTable
              columns={extColumns}
              rows={tree.extensions}
              rowKey={(e) => e.ext}
              caption="Size by file extension"
              density="compact"
              defaultSort={{ key: "bytes", dir: "desc" }}
              empty={{ title: "No files", description: "The scan found no files." }}
            />
          </Card>
        </>
      )}

      {reviewing && (
        <ReviewCleanModal
          items={[finding(reviewing)]}
          onConfirm={(ids) => remove(ids)}
          onClose={() => setReviewing(null)}
        />
      )}
    </Stack>
  );
}

/** Items and bytes scanned, and a bar against the expected total when one is known. */
function ScanStatus({ progress, estimate }: { progress: ScanProgress | null; estimate: number | null }) {
  const items = progress?.entries ?? 0;
  const bytes = progress?.bytes ?? 0;
  // The total is an estimate, so the bar holds short of full until the scan ends.
  const done = estimate ? Math.min(99, percent(bytes, estimate)) : 0;
  return (
    <Callout tone="info" title="Scanning">
      <Stack gap={2}>
        <Stack direction="row" align="center" justify="space-between" gap={3} wrap>
          <span>
            {items.toLocaleString()} items scanned, {formatBytes(bytes)} scanned
          </span>
          {estimate !== null && estimate > 0 && (
            <Meter variant="inline" label="Scan progress" percent={done} valueLabel={`${Math.floor(done)}%`} warningAt={101} criticalAt={101} />
          )}
        </Stack>
        {estimate ? (
          <span>
            Scanned {formatBytes(bytes)} of about {formatBytes(estimate)} total; about {formatBytes(Math.max(0, estimate - bytes))} left
          </span>
        ) : (
          <span>Total size unknown until this folder has been scanned once, so there is no progress bar yet.</span>
        )}
      </Stack>
    </Callout>
  );
}

function CellShape({ cell, total, selected, onClick, onHover }: { cell: Cell; total: number; selected: boolean; onClick: (n: TreeNode) => void; onHover: (n: TreeNode | null) => void }) {
  const { node, rect, frame } = cell;
  const showName = rect.w >= 60 && rect.h >= 16;
  const showSize = !frame && rect.h >= 32;
  const interactive = node.path !== "";
  return (
    <g
      role={interactive ? "button" : undefined}
      aria-label={interactive ? `Treemap ${node.name}` : undefined}
      aria-pressed={interactive ? selected : undefined}
      onClick={() => onClick(node)}
      onMouseEnter={() => onHover(node)}
      cursor={interactive ? "pointer" : "default"}
    >
      <title>{`${node.path || node.name}\n${formatBytes(node.bytes)}, ${formatPercent(percent(node.bytes, total))} of the map`}</title>
      <rect
        x={rect.x}
        y={rect.y}
        width={rect.w}
        height={rect.h}
        fill={colorOf(node)}
        fillOpacity={node.path === "" ? 0.55 : 1}
        stroke={selected ? "currentColor" : "rgba(0,0,0,0.4)"}
        strokeWidth={selected ? 3 : 1}
      />
      {showName && (
        <text x={rect.x + 4} y={rect.y + 11} fontSize="11" fill="#fff" pointerEvents="none">
          {fit(node.name, rect.w - 8)}
        </text>
      )}
      {showName && showSize && (
        <text x={rect.x + 4} y={rect.y + 25} fontSize="10" fill="#fff" fillOpacity="0.85" pointerEvents="none">
          {fit(formatBytes(node.bytes), rect.w - 8)}
        </text>
      )}
    </g>
  );
}
