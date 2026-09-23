import { useEffect, useState } from "react";
import { Button, Callout, DataTable, IconButton, Modal, ModalFooter, SearchInput, Tabs, TextInput } from "../../../ui";
import { Bookmark as BookmarkIcon, ChevronDown, ChevronUp, Clock3, Copy, Eye, EyeOff, KeyRound, Pencil, Trash2 } from "../../../ui/icons";
import {
  Bookmark,
  Credential,
  HistoryEntry,
  browserBookmarkDelete,
  browserBookmarkUpdate,
  browserBookmarksReorder,
  browserCredentialDelete,
  browserCredentialReveal,
  browserCredentialsList,
  browserHistoryClear,
  browserHistoryDelete,
  browserHistorySearch,
} from "../api";
import { errorMessage, hostOf } from "../utils";

export type LibraryTab = "bookmarks" | "passwords" | "history";

interface Props {
  initial: LibraryTab;
  bookmarks: Bookmark[];
  onBookmarksChanged: () => void;
  onOpen: (url: string) => void;
  onClose: () => void;
}

const TABS = [
  { id: "bookmarks", label: "Bookmarks", icon: BookmarkIcon },
  { id: "passwords", label: "Passwords", icon: KeyRound },
  { id: "history", label: "History", icon: Clock3 },
];

/** Bookmarks, saved logins and history, in one place. */
export function LibraryModal({ initial, bookmarks, onBookmarksChanged, onOpen, onClose }: Props) {
  const [tab, setTab] = useState<LibraryTab>(initial);
  const [error, setError] = useState<string | null>(null);

  return (
    <Modal title="Library" icon={BookmarkIcon} size="lg" onClose={onClose} footer={<ModalFooter><Button onClick={onClose}>Close</Button></ModalFooter>}>
      <Tabs tabs={TABS} active={tab} onChange={(id) => setTab(id as LibraryTab)} ariaLabel="Library sections" />
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <div className="browser-library">
        {tab === "bookmarks" && <BookmarksPane bookmarks={bookmarks} onChanged={onBookmarksChanged} onOpen={onOpen} onError={setError} />}
        {tab === "passwords" && <PasswordsPane onError={setError} />}
        {tab === "history" && <HistoryPane onOpen={onOpen} onError={setError} />}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- bookmarks

function BookmarksPane({
  bookmarks,
  onChanged,
  onOpen,
  onError,
}: {
  bookmarks: Bookmark[];
  onChanged: () => void;
  onOpen: (url: string) => void;
  onError: (m: string) => void;
}) {
  const [editing, setEditing] = useState<Bookmark | null>(null);
  const [title, setTitle] = useState("");
  const [url, setUrl] = useState("");

  function startEdit(b: Bookmark) {
    setEditing(b);
    setTitle(b.title);
    setUrl(b.url);
  }

  async function saveEdit() {
    if (!editing) return;
    try {
      await browserBookmarkUpdate(editing.id, url.trim(), title.trim());
      setEditing(null);
      onChanged();
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  async function move(b: Bookmark, dir: -1 | 1) {
    const ids = bookmarks.map((x) => x.id);
    const i = ids.indexOf(b.id);
    const j = i + dir;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    try {
      await browserBookmarksReorder(ids);
      onChanged();
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  async function remove(b: Bookmark) {
    try {
      await browserBookmarkDelete(b.id);
      onChanged();
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  return (
    <>
      {editing && (
        <div className="browser-library-edit">
          <TextInput inputSize="sm" aria-label="Bookmark title" placeholder="Title" value={title} onChange={(e) => setTitle(e.target.value)} />
          <TextInput inputSize="sm" mono aria-label="Bookmark URL" placeholder="https://" value={url} onChange={(e) => setUrl(e.target.value)} />
          <Button size="sm" variant="primary" onClick={() => void saveEdit()}>
            Save
          </Button>
          <Button size="sm" onClick={() => setEditing(null)}>
            Cancel
          </Button>
        </div>
      )}
      <DataTable<Bookmark>
        rows={bookmarks}
        rowKey={(b) => b.id}
        onRowDoubleClick={(b) => onOpen(b.url)}
        empty={{ icon: BookmarkIcon, title: "No bookmarks yet", description: "Press the star in the address bar, or Ctrl+D, to keep a page." }}
        columns={[
          { key: "title", header: "Title", render: (b) => b.title || hostOf(b.url), sortValue: (b) => b.title },
          { key: "url", header: "URL", render: (b) => b.url, mono: true, priority: 2 },
          {
            key: "actions",
            header: "",
            headerLabel: "Actions",
            align: "end",
            render: (b, i) => (
              <span className="browser-library-actions">
                <IconButton icon={ChevronUp} label="Move up" size="sm" disabled={i === 0} onClick={() => void move(b, -1)} />
                <IconButton icon={ChevronDown} label="Move down" size="sm" disabled={i === bookmarks.length - 1} onClick={() => void move(b, 1)} />
                <IconButton icon={Pencil} label="Edit" size="sm" onClick={() => startEdit(b)} />
                <IconButton icon={Trash2} label="Delete" size="sm" variant="danger" onClick={() => void remove(b)} />
              </span>
            ),
          },
        ]}
      />
    </>
  );
}

// ---------------------------------------------------------------- passwords

function PasswordsPane({ onError }: { onError: (m: string) => void }) {
  const [rows, setRows] = useState<Credential[]>([]);
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [filter, setFilter] = useState("");

  const load = () => browserCredentialsList().then(setRows).catch((e) => onError(errorMessage(e)));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function toggle(c: Credential) {
    if (revealed[c.id] !== undefined) {
      setRevealed((r) => {
        const next = { ...r };
        delete next[c.id];
        return next;
      });
      return;
    }
    try {
      const pw = await browserCredentialReveal(c.id);
      setRevealed((r) => ({ ...r, [c.id]: pw }));
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  async function copy(c: Credential) {
    try {
      const pw = revealed[c.id] ?? (await browserCredentialReveal(c.id));
      await navigator.clipboard.writeText(pw);
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  async function remove(c: Credential) {
    try {
      await browserCredentialDelete(c.id);
      await load();
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  const needle = filter.trim().toLowerCase();
  const shown = needle ? rows.filter((c) => c.origin.toLowerCase().includes(needle) || c.username.toLowerCase().includes(needle)) : rows;

  return (
    <>
      <div className="browser-library-toolbar">
        <SearchInput value={filter} onChange={setFilter} placeholder="Filter by site or username" aria-label="Filter logins" />
      </div>
      <DataTable<Credential>
        rows={shown}
        rowKey={(c) => c.id}
        empty={{ icon: KeyRound, title: "No saved logins", description: "Sign in to a site and choose Save when asked." }}
        columns={[
          { key: "site", header: "Site", render: (c) => hostOf(c.origin), sortValue: (c) => c.origin },
          { key: "user", header: "Username", render: (c) => c.username || "—", mono: true },
          {
            key: "password",
            header: "Password",
            render: (c) => <span className="u-mono">{revealed[c.id] !== undefined ? revealed[c.id] : "••••••••"}</span>,
          },
          {
            key: "actions",
            header: "",
            headerLabel: "Actions",
            align: "end",
            render: (c) => (
              <span className="browser-library-actions">
                <IconButton icon={revealed[c.id] !== undefined ? EyeOff : Eye} label={revealed[c.id] !== undefined ? "Hide" : "Show"} size="sm" onClick={() => void toggle(c)} />
                <IconButton icon={Copy} label="Copy password" size="sm" onClick={() => void copy(c)} />
                <IconButton icon={Trash2} label="Delete" size="sm" variant="danger" onClick={() => void remove(c)} />
              </span>
            ),
          },
        ]}
      />
    </>
  );
}

// ------------------------------------------------------------------ history

function HistoryPane({ onOpen, onError }: { onOpen: (url: string) => void; onError: (m: string) => void }) {
  const [rows, setRows] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const t = setTimeout(() => {
      browserHistorySearch(query, 200).then(setRows).catch((e) => onError(errorMessage(e)));
    }, 80);
    return () => clearTimeout(t);
  }, [query, onError]);

  async function remove(h: HistoryEntry) {
    try {
      await browserHistoryDelete(h.id);
      setRows((r) => r.filter((x) => x.id !== h.id));
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  async function clearAll() {
    try {
      await browserHistoryClear();
      setRows([]);
    } catch (e) {
      onError(errorMessage(e));
    }
  }

  return (
    <>
      <div className="browser-library-toolbar">
        <SearchInput value={query} onChange={setQuery} placeholder="Search history" aria-label="Search history" />
        <Button size="sm" variant="danger" icon={Trash2} disabled={rows.length === 0} onClick={() => void clearAll()}>
          Clear history
        </Button>
      </div>
      <DataTable<HistoryEntry>
        rows={rows}
        rowKey={(h) => h.id}
        onRowDoubleClick={(h) => onOpen(h.url)}
        empty={{ icon: Clock3, title: "No history", description: "Pages you visit show up here and in the address bar." }}
        columns={[
          { key: "title", header: "Page", render: (h) => h.title || hostOf(h.url), sortValue: (h) => h.title },
          { key: "url", header: "URL", render: (h) => h.url, mono: true, priority: 2 },
          { key: "visits", header: "Visits", render: (h) => h.visits, align: "end", sortValue: (h) => h.visits },
          {
            key: "when",
            header: "Last visit",
            render: (h) => new Date(h.visited_at).toLocaleString(),
            sortValue: (h) => h.visited_at,
          },
          {
            key: "actions",
            header: "",
            headerLabel: "Actions",
            align: "end",
            render: (h) => <IconButton icon={Trash2} label="Remove" size="sm" variant="danger" onClick={() => void remove(h)} />,
          },
        ]}
      />
    </>
  );
}
