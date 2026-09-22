import { useCallback, useEffect, useState } from "react";
import { useConfirm } from "../../../app/ConfirmContext";
import {
  ActionGroup,
  Badge,
  Button,
  Callout,
  Column,
  DataTable,
  IconButton,
  SearchInput,
  Section,
  Stack,
  Toolbar,
} from "../../../ui";
import { ChevronDown, SearchX, Trash2 } from "../../../ui/icons";
import { deleteKeys, scanKeys, type KeyView, type RedisConn } from "../api";
import { formatTtl, typeTone } from "../utils";
import ValueModal from "./ValueModal";

const PAGE = 100;
const DEBOUNCE_MS = 300;

/**
 * SCAN-based key browser. Pages are appended (Load more) rather than
 * replaced because SCAN's cursor is a position in the keyspace, not an
 * offset: there is no way to re-request "page 2" on its own.
 */
export default function KeysTab({ conn }: { conn: RedisConn }) {
  const confirmAction = useConfirm();
  const [pattern, setPattern] = useState("*");
  const [rows, setRows] = useState<KeyView[]>([]);
  const [cursor, setCursor] = useState("0");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);

  const load = useCallback(
    async (from: string, append: boolean) => {
      setLoading(true);
      setError(null);
      try {
        const page = await scanKeys(conn, pattern || "*", from, PAGE);
        setRows((prev) => {
          const next = append ? [...prev] : [];
          const seen = new Set(next.map((r) => r.key));
          for (const k of page.keys) if (!seen.has(k.key)) next.push(k);
          return next;
        });
        setCursor(page.cursor);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    },
    [conn, pattern],
  );

  // Debounced fresh scan whenever the pattern or connection changes.
  useEffect(() => {
    const id = setTimeout(() => void load("0", false), DEBOUNCE_MS);
    return () => clearTimeout(id);
  }, [load]);

  async function handleDelete(key: string) {
    const ok = await confirmAction(`Delete "${key}"? This cannot be undone.`, {
      title: "Delete key?",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    setBusyKey(key);
    try {
      await deleteKeys(conn, [key]);
      setRows((prev) => prev.filter((r) => r.key !== key));
      if (open === key) setOpen(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyKey(null);
    }
  }

  const columns: Column<KeyView>[] = [
    {
      key: "key",
      header: "Key",
      priority: 1,
      mono: true,
      sortValue: (r) => r.key,
      render: (r) => r.key,
    },
    {
      key: "type",
      header: "Type",
      priority: 1,
      width: "1px",
      sortValue: (r) => r.type,
      render: (r) => <Badge tone={typeTone(r.type)}>{r.type}</Badge>,
    },
    {
      key: "ttl",
      header: "TTL",
      priority: 2,
      width: "1px",
      sortValue: (r) => (r.ttl < 0 ? Number.MAX_SAFE_INTEGER : r.ttl),
      render: (r) => (r.ttl === -1 ? <span className="u-muted">no expiry</span> : formatTtl(r.ttl)),
    },
    {
      key: "actions",
      header: "",
      priority: 1,
      align: "end",
      width: "1px",
      render: (r) => (
        <ActionGroup>
          <IconButton
            icon={Trash2}
            label="Delete key"
            variant="danger"
            loading={busyKey === r.key}
            onClick={() => handleDelete(r.key)}
          />
        </ActionGroup>
      ),
    },
  ];

  const more = cursor !== "0";

  return (
    <Section
      title="Keys"
      padding="none"
      description="Click a key to see its value. Patterns use Redis glob syntax: user:*, *session*."
      toolbar={
        <Toolbar
          end={
            <span className="u-muted">
              {rows.length} key{rows.length === 1 ? "" : "s"}
              {more ? " so far" : ""}
            </span>
          }
        >
          <PatternInput value={pattern} onChange={setPattern} />
        </Toolbar>
      }
      footer={
        more || error ? (
          <Stack gap={3}>
            {error && <Callout tone="danger">{error}</Callout>}
            {more && (
              <Button
                variant="secondary"
                icon={ChevronDown}
                loading={loading}
                onClick={() => void load(cursor, true)}
              >
                Load more
              </Button>
            )}
          </Stack>
        ) : undefined
      }
    >
      <DataTable
        columns={columns}
        rows={rows}
        rowKey={(r) => r.key}
        loading={loading && rows.length === 0}
        onRowClick={(r) => setOpen(r.key)}
        caption="Keys matching the pattern"
        empty={{
          icon: SearchX,
          title: "No keys match",
          description: `Nothing in this database matches "${pattern || "*"}".`,
        }}
      />
      {open && (
        <ValueModal
          conn={conn}
          keyName={open}
          onDelete={() => void handleDelete(open)}
          onClose={() => setOpen(null)}
        />
      )}
    </Section>
  );
}

function PatternInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  // SearchInput's clear button empties the value; an empty pattern means "*".
  return (
    <SearchInput
      value={value}
      onChange={onChange}
      placeholder="Pattern, e.g. user:* (default *)"
      width={320}
    />
  );
}
