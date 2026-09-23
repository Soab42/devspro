import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { Button, Icon, IconButton } from "../../../ui";
import { Bookmark as BookmarkIcon, ChevronDown, ChevronUp, Download, KeyRound, X } from "../../../ui/icons";
import type { Bookmark, CredentialEvent, DownloadEvent } from "../api";
import { hostOf } from "../utils";

// ---------------------------------------------------------------- bookmarks

interface BookmarksBarProps {
  bookmarks: Bookmark[];
  onOpen: (url: string, newTab: boolean) => void;
}

/** The strip under the toolbar. Ctrl/middle-click opens in a new tab. */
export function BookmarksBar({ bookmarks, onOpen }: BookmarksBarProps) {
  if (bookmarks.length === 0) return null;
  return (
    <div className="browser-bookmarks" aria-label="Bookmarks">
      {bookmarks.map((b) => (
        <button
          key={b.id}
          type="button"
          className="browser-bookmark"
          title={b.url}
          onClick={(e) => onOpen(b.url, e.ctrlKey || e.metaKey)}
          onMouseUp={(e) => {
            if (e.button === 1) {
              e.preventDefault();
              onOpen(b.url, true);
            }
          }}
        >
          <Icon icon={BookmarkIcon} size="xs" />
          <span className="browser-bookmark-label">{b.title || hostOf(b.url)}</span>
        </button>
      ))}
    </div>
  );
}

// --------------------------------------------------------------------- find

interface FindBarProps {
  onFind: (text: string, backwards: boolean) => void;
  onClose: () => void;
}

export function FindBar({ onFind, onClose }: FindBarProps) {
  const [text, setText] = useState("");
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
  }, []);

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault();
      onFind(text, e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onFind("", false);
      onClose();
    }
  }

  return (
    <div className="browser-bar browser-find" role="search" aria-label="Find in page">
      <input
        ref={ref}
        className="browser-find-input"
        type="text"
        placeholder="Find in page"
        aria-label="Find in page"
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          onFind(e.target.value, false);
        }}
        onKeyDown={onKeyDown}
      />
      <IconButton icon={ChevronUp} label="Previous match" size="sm" onClick={() => onFind(text, true)} />
      <IconButton icon={ChevronDown} label="Next match" size="sm" onClick={() => onFind(text, false)} />
      <IconButton
        icon={X}
        label="Close find"
        size="sm"
        onClick={() => {
          onFind("", false);
          onClose();
        }}
      />
    </div>
  );
}

// ------------------------------------------------------------ save password

interface SavePasswordBarProps {
  pending: CredentialEvent;
  onSave: () => void;
  onNever: () => void;
  onDismiss: () => void;
}

export function SavePasswordBar({ pending, onSave, onNever, onDismiss }: SavePasswordBarProps) {
  return (
    <div className="browser-bar browser-save" role="status">
      <Icon icon={KeyRound} size="sm" />
      <span className="browser-bar-text">
        {pending.update ? "Update password" : "Save password"} for <strong>{hostOf(pending.origin)}</strong>
        {pending.username && (
          <>
            {" "}
            as <strong>{pending.username}</strong>
          </>
        )}
        ?
      </span>
      <div className="browser-bar-actions">
        <Button size="sm" variant="primary" onClick={onSave}>
          {pending.update ? "Update" : "Save"}
        </Button>
        <Button size="sm" onClick={onNever}>
          Never for this site
        </Button>
        <IconButton icon={X} label="Not now" size="sm" onClick={onDismiss} />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- downloads

interface DownloadsBarProps {
  downloads: DownloadEvent[];
  onReveal: (path: string) => void;
  onClear: () => void;
}

export function DownloadsBar({ downloads, onReveal, onClear }: DownloadsBarProps) {
  if (downloads.length === 0) return null;
  return (
    <div className="browser-bar browser-downloads" role="status">
      <Icon icon={Download} size="sm" />
      <div className="browser-downloads-list">
        {downloads.map((d) => {
          const name = d.path?.split("/").pop() ?? d.url;
          const status = d.state === "started" ? "downloading…" : d.success ? "done" : "failed";
          return (
            <span key={d.url + (d.path ?? "")} className="browser-download">
              <span className="browser-download-name" title={d.path ?? d.url}>
                {name}
              </span>
              <span className="browser-download-status">{status}</span>
              {d.state === "finished" && d.success && d.path && (
                <Button size="sm" variant="ghost" onClick={() => onReveal(d.path as string)}>
                  Show
                </Button>
              )}
            </span>
          );
        })}
      </div>
      <IconButton icon={X} label="Clear downloads" size="sm" onClick={onClear} />
    </div>
  );
}
