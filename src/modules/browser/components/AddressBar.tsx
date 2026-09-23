import { FormEvent, KeyboardEvent, useEffect, useRef, useState } from "react";
import { Icon } from "../../../ui";
import { Bookmark as BookmarkIcon, Clock3, Globe, KeyRound, Lock, Search, Star } from "../../../ui/icons";
import { Bookmark, Credential, HistoryEntry, browserHistorySearch } from "../api";
import { Skin, Suggestion, buildSuggestions, displayUrl, hostOf, isSecure, resolveInput } from "../utils";

interface Props {
  url: string;
  skin: Skin;
  disabled: boolean;
  bookmarks: Bookmark[];
  bookmarked: boolean;
  logins: Credential[];
  onNavigate: (url: string) => void;
  onToggleBookmark: () => void;
  onFill: (credentialId: string) => void;
  /** True while the suggestions list or the logins popover is showing. */
  onOverlay: (open: boolean) => void;
  inputRef: React.RefObject<HTMLInputElement | null>;
}

const KIND_ICON = { bookmark: BookmarkIcon, history: Clock3, search: Search, url: Globe } as const;

/**
 * The omnibox. Typing searches bookmarks and history and shows up to eight
 * suggestions; arrows move, Enter picks, Escape restores the page URL. The
 * star bookmarks the page and the key lists saved logins for this site.
 */
export function AddressBar({ url, skin, disabled, bookmarks, bookmarked, logins, onNavigate, onToggleBookmark, onFill, onOverlay, inputRef }: Props) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [highlight, setHighlight] = useState(-1);
  const [loginsOpen, setLoginsOpen] = useState(false);
  const loginsRef = useRef<HTMLDivElement>(null);
  const requestSeq = useRef(0);

  useEffect(() => {
    if (!editing) setText(url);
  }, [url, editing]);

  // Debounced history lookup while typing. The sequence number drops answers
  // that arrive after a newer query was sent.
  useEffect(() => {
    if (!editing || !text.trim()) {
      setHistory([]);
      return;
    }
    const seq = ++requestSeq.current;
    const timer = setTimeout(() => {
      browserHistorySearch(text, 8)
        .then((rows) => {
          if (seq === requestSeq.current) setHistory(rows);
        })
        .catch(() => undefined);
    }, 80);
    return () => clearTimeout(timer);
  }, [text, editing]);

  useEffect(() => {
    if (!loginsOpen) return;
    const close = (e: Event) => {
      if (!loginsRef.current?.contains(e.target as Node)) setLoginsOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [loginsOpen]);

  const suggestions: Suggestion[] = editing ? buildSuggestions(text, bookmarks, history) : [];
  const overlay = suggestions.length > 0 || loginsOpen;
  useEffect(() => {
    onOverlay(overlay);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [overlay]);

  function commit(target: string) {
    setEditing(false);
    setHighlight(-1);
    inputRef.current?.blur();
    onNavigate(target);
  }

  function submit(e: FormEvent) {
    e.preventDefault();
    const pick = highlight >= 0 ? suggestions[highlight] : null;
    commit(pick ? pick.url : resolveInput(text));
  }

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown" && suggestions.length) {
      e.preventDefault();
      setHighlight((h) => (h + 1) % suggestions.length);
    } else if (e.key === "ArrowUp" && suggestions.length) {
      e.preventDefault();
      setHighlight((h) => (h <= 0 ? suggestions.length - 1 : h - 1));
    } else if (e.key === "Escape") {
      e.preventDefault();
      setEditing(false);
      setHighlight(-1);
      setText(url);
      inputRef.current?.blur();
    }
  }

  const shown = editing ? text : url ? (skin === "safari" ? hostOf(url) : displayUrl(url)) : "";

  return (
    <form className="browser-address" onSubmit={submit} role="search">
      <span className="browser-address-icon" aria-hidden="true">
        <Icon icon={url && isSecure(url) ? Lock : Globe} size="xs" />
      </span>
      <input
        ref={inputRef}
        className="browser-address-input"
        type="text"
        aria-label="Address"
        aria-autocomplete="list"
        aria-expanded={suggestions.length > 0}
        placeholder="Search or enter address"
        value={shown}
        spellCheck={false}
        autoComplete="off"
        disabled={disabled}
        onFocus={(e) => {
          setEditing(true);
          setText(url);
          requestAnimationFrame(() => e.target.select());
        }}
        onBlur={() => {
          // Let a click on a suggestion land before the list goes away.
          setTimeout(() => {
            setEditing(false);
            setHighlight(-1);
          }, 120);
        }}
        onChange={(e) => {
          setText(e.target.value);
          setHighlight(-1);
        }}
        onKeyDown={onKeyDown}
      />
      {url && logins.length > 0 && (
        <div ref={loginsRef} className="browser-address-tool">
          <button
            type="button"
            className="browser-address-button"
            aria-label={`Saved logins for ${hostOf(url)}`}
            title="Saved logins"
            aria-expanded={loginsOpen}
            onClick={() => setLoginsOpen((o) => !o)}
          >
            <Icon icon={KeyRound} size="xs" />
          </button>
          {loginsOpen && (
            <div className="browser-popover" role="menu">
              <div className="browser-popover-title">Fill login</div>
              {logins.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  role="menuitem"
                  className="browser-popover-item"
                  onClick={() => {
                    setLoginsOpen(false);
                    onFill(c.id);
                  }}
                >
                  <Icon icon={KeyRound} size="xs" />
                  <span>{c.username || "(no username)"}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      {url && (
        <button
          type="button"
          className={bookmarked ? "browser-address-button browser-address-star-on" : "browser-address-button"}
          aria-label={bookmarked ? "Remove bookmark" : "Bookmark this page"}
          title={bookmarked ? "Remove bookmark (Ctrl+D)" : "Bookmark this page (Ctrl+D)"}
          aria-pressed={bookmarked}
          onClick={onToggleBookmark}
        >
          <Icon icon={Star} size="xs" />
        </button>
      )}
      {suggestions.length > 0 && (
        <ul className="browser-suggestions" role="listbox">
          {suggestions.map((s, i) => (
            <li
              key={`${s.kind}:${s.url}`}
              role="option"
              aria-selected={i === highlight}
              className={i === highlight ? "browser-suggestion browser-suggestion-active" : "browser-suggestion"}
              onMouseDown={(e) => {
                e.preventDefault();
                commit(s.url);
              }}
              onMouseEnter={() => setHighlight(i)}
            >
              <Icon icon={KIND_ICON[s.kind]} size="xs" />
              <span className="browser-suggestion-title">{s.title}</span>
              {s.kind !== "search" && s.kind !== "url" && <span className="browser-suggestion-url">{displayUrl(s.url)}</span>}
            </li>
          ))}
        </ul>
      )}
    </form>
  );
}
