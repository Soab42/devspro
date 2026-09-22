import { FormEvent, KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useModule } from "../../../app/ModuleRegistry";
import { Callout, EmptyState, Icon, IconButton, Page, SegmentedControl } from "../../../ui";
import {
  AppWindow,
  ChevronLeft,
  ChevronRight,
  Compass,
  ExternalLink,
  Flame,
  Globe,
  Lock,
  Plus,
  RotateCw,
  X,
  ZoomIn,
  ZoomOut,
} from "../../../ui/icons";
import {
  Bounds,
  LOAD_EVENT,
  LoadEvent,
  NEW_WINDOW_EVENT,
  NewWindowEvent,
  TITLE_EVENT,
  TitleEvent,
  browserBack,
  browserClose,
  browserForward,
  browserNavigate,
  browserOpen,
  browserReload,
  browserSetBounds,
  browserSetVisible,
  browserSetZoom,
  browserStop,
} from "../api";
import { DEFAULT_URL, Skin, displayUrl, errorMessage, hostOf, isSecure, nextZoom, resolveInput, tabLabel } from "../utils";
import "./BrowserPage.css";

interface Tab {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  zoom: number;
}

const SKIN_KEY = "devspro.browser.skin";

function loadSkin(): Skin {
  try {
    const v = localStorage.getItem(SKIN_KEY);
    if (v === "chrome" || v === "safari" || v === "firefox") return v;
  } catch {
    /* private mode or blocked storage: default */
  }
  return "chrome";
}

/**
 * Tabs outlive the page. The native webviews are owned by the window, not by
 * React, so when the user visits Docker and comes back, the same tabs must be
 * re-adopted rather than re-created — a half-filled form on a page shouldn't
 * vanish because a sidebar link was clicked. This store is that memory.
 */
const store: { tabs: Tab[]; activeId: string | null; skin: Skin } = {
  tabs: [],
  activeId: null,
  skin: loadSkin(),
};

function measure(el: HTMLElement | null): Bounds | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) return null;
  return { x: r.left, y: r.top, width: r.width, height: r.height };
}

/**
 * A small browser with three familiar faces.
 *
 * The page content is a native child webview (see modules/browser/mod.rs), so
 * this component only draws the frame: tab strip, address bar, zoom. The skin
 * changes the arrangement and shapes of that frame — Chrome's rounded tabs
 * over a pill address bar, Safari's unified toolbar with a centred, host-only
 * address field and tabs underneath, Firefox's square tabs with an accent
 * top edge — not the engine, which is always WebKitGTK.
 */
export default function BrowserPage() {
  const module = useModule("browser");
  const [tabs, setTabs] = useState<Tab[]>(store.tabs);
  const [activeId, setActiveId] = useState<string | null>(store.activeId);
  const [skin, setSkin] = useState<Skin>(store.skin);
  const [address, setAddress] = useState("");
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const addressRef = useRef<HTMLInputElement>(null);

  const active = tabs.find((t) => t.id === activeId) ?? null;

  useEffect(() => {
    store.tabs = tabs;
    store.activeId = activeId;
  }, [tabs, activeId]);

  useEffect(() => {
    store.skin = skin;
    try {
      localStorage.setItem(SKIN_KEY, skin);
    } catch {
      /* fine: the choice just won't persist */
    }
  }, [skin]);

  const fit = useCallback((id: string) => {
    const bounds = measure(contentRef.current);
    if (bounds) browserSetBounds(id, bounds).catch(() => undefined);
  }, []);

  // Keep the native view glued to the content area through window resizes,
  // sidebar collapse and skin changes (which alter the toolbar height).
  useEffect(() => {
    if (!activeId) return;
    fit(activeId);
    const el = contentRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => fit(activeId));
    ro.observe(el);
    ro.observe(document.documentElement);
    const onResize = () => fit(activeId);
    window.addEventListener("resize", onResize);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", onResize);
    };
  }, [activeId, skin, fit]);

  // Re-adopt on mount, hide on unmount: a native view would otherwise sit on
  // top of whatever module the user navigated to.
  useEffect(() => {
    const current = store.activeId;
    if (current) {
      browserSetVisible(current, true).catch(() => undefined);
    }
    return () => {
      for (const t of store.tabs) browserSetVisible(t.id, false).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    const unlisteners = [
      listen<TitleEvent>(TITLE_EVENT, (e) => {
        setTabs((ts) => ts.map((t) => (t.id === e.payload.id ? { ...t, title: e.payload.title } : t)));
      }),
      listen<LoadEvent>(LOAD_EVENT, (e) => {
        const { id, url, state } = e.payload;
        setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, url, loading: state === "started" } : t)));
      }),
      listen<NewWindowEvent>(NEW_WINDOW_EVENT, (e) => {
        void openTab(e.payload.url);
      }),
    ];
    return () => {
      for (const p of unlisteners) p.then((un) => un()).catch(() => undefined);
    };
    // openTab is stable enough for the lifetime of the page; re-subscribing on
    // every render would race with events fired between unlisten and listen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!editing) setAddress(active?.url ?? "");
  }, [active?.url, editing]);

  async function activate(id: string) {
    const previous = store.activeId;
    if (previous && previous !== id) await browserSetVisible(previous, false).catch(() => undefined);
    setActiveId(id);
    await browserSetVisible(id, true).catch(() => undefined);
    fit(id);
  }

  async function openTab(url: string = DEFAULT_URL) {
    const id = crypto.randomUUID();
    const tab: Tab = { id, url, title: "", loading: true, zoom: 1 };
    setTabs((ts) => [...ts, tab]);
    store.tabs = [...store.tabs, tab];
    const previous = store.activeId;
    if (previous) await browserSetVisible(previous, false).catch(() => undefined);
    setActiveId(id);
    store.activeId = id;
    const bounds = measure(contentRef.current) ?? { x: 0, y: 0, width: 800, height: 600 };
    try {
      await browserOpen(id, url, bounds);
      fit(id);
    } catch (e) {
      setError(errorMessage(e));
      setTabs((ts) => ts.filter((t) => t.id !== id));
      store.tabs = store.tabs.filter((t) => t.id !== id);
      if (previous) await activate(previous);
      else setActiveId(null);
    }
  }

  async function closeTab(id: string) {
    const remaining = tabs.filter((t) => t.id !== id);
    const index = tabs.findIndex((t) => t.id === id);
    setTabs(remaining);
    await browserClose(id).catch(() => undefined);
    if (activeId === id) {
      const next = remaining[Math.min(index, remaining.length - 1)] ?? null;
      if (next) await activate(next.id);
      else setActiveId(null);
    }
  }

  function submitAddress(e: FormEvent) {
    e.preventDefault();
    const url = resolveInput(address);
    setEditing(false);
    addressRef.current?.blur();
    if (active) {
      setTabs((ts) => ts.map((t) => (t.id === active.id ? { ...t, loading: true } : t)));
      browserNavigate(active.id, url).catch((err) => setError(errorMessage(err)));
    } else {
      void openTab(url);
    }
  }

  async function zoom(direction: -1 | 1) {
    if (!active) return;
    const factor = nextZoom(active.zoom, direction);
    await browserSetZoom(active.id, factor).catch((err) => setError(errorMessage(err)));
    setTabs((ts) => ts.map((t) => (t.id === active.id ? { ...t, zoom: factor } : t)));
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (!(e.ctrlKey || e.metaKey)) return;
    if (e.key === "l") {
      e.preventDefault();
      addressRef.current?.focus();
      addressRef.current?.select();
    } else if (e.key === "t") {
      e.preventDefault();
      void openTab();
    } else if (e.key === "w" && active) {
      e.preventDefault();
      void closeTab(active.id);
    }
  }

  const shownAddress = editing ? address : active ? (skin === "safari" ? hostOf(active.url) : displayUrl(active.url)) : "";

  return (
    <Page
      title={module.label}
      icon={module.icon}
      actions={
        <SegmentedControl
          ariaLabel="Browser style"
          size="sm"
          value={skin}
          onChange={setSkin}
          options={[
            { value: "chrome", label: "Chrome", icon: Globe },
            { value: "safari", label: "Safari", icon: Compass },
            { value: "firefox", label: "Firefox", icon: Flame },
          ]}
        />
      }
    >
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <div className={`browser browser-skin-${skin}`} onKeyDown={handleKeyDown}>
        <div className="browser-tabs" role="tablist" aria-label="Open tabs">
          {tabs.map((t) => (
            <div key={t.id} className={t.id === activeId ? "browser-tab browser-tab-active" : "browser-tab"}>
              <button
                type="button"
                role="tab"
                aria-selected={t.id === activeId}
                className="browser-tab-button"
                onClick={() => void activate(t.id)}
                title={t.url}
              >
                <span className="browser-tab-favicon">
                  <Icon icon={t.loading ? RotateCw : Globe} size="xs" className={t.loading ? "browser-spin" : undefined} />
                </span>
                <span className="browser-tab-label">{tabLabel(t.title, t.url)}</span>
              </button>
              <button type="button" className="browser-tab-close" aria-label={`Close ${tabLabel(t.title, t.url)}`} onClick={() => void closeTab(t.id)}>
                <Icon icon={X} size="xs" />
              </button>
            </div>
          ))}
          <button type="button" className="browser-tab-new" aria-label="New tab" onClick={() => void openTab()}>
            <Icon icon={Plus} size="sm" />
          </button>
        </div>

        <div className="browser-nav">
          <div className="browser-nav-group">
            <IconButton icon={ChevronLeft} label="Back" size="sm" disabled={!active} onClick={() => active && browserBack(active.id)} />
            <IconButton icon={ChevronRight} label="Forward" size="sm" disabled={!active} onClick={() => active && browserForward(active.id)} />
            <IconButton
              icon={active?.loading ? X : RotateCw}
              label={active?.loading ? "Stop" : "Reload"}
              size="sm"
              disabled={!active}
              onClick={() => active && (active.loading ? browserStop(active.id) : browserReload(active.id))}
            />
          </div>
          <form className="browser-address" onSubmit={submitAddress}>
            <span className="browser-address-icon" aria-hidden="true">
              <Icon icon={active && isSecure(active.url) ? Lock : Globe} size="xs" />
            </span>
            <input
              ref={addressRef}
              className="browser-address-input"
              type="text"
              aria-label="Address"
              placeholder="Search or enter address"
              value={shownAddress}
              spellCheck={false}
              autoComplete="off"
              onFocus={(e) => {
                setEditing(true);
                setAddress(active?.url ?? "");
                requestAnimationFrame(() => e.target.select());
              }}
              onBlur={() => setEditing(false)}
              onChange={(e) => setAddress(e.target.value)}
            />
          </form>
          <div className="browser-nav-group">
            <IconButton icon={ZoomOut} label="Zoom out" size="sm" disabled={!active} onClick={() => void zoom(-1)} />
            {active && active.zoom !== 1 && <span className="browser-zoom u-tnum">{Math.round(active.zoom * 100)}%</span>}
            <IconButton icon={ZoomIn} label="Zoom in" size="sm" disabled={!active} onClick={() => void zoom(1)} />
            <IconButton
              icon={ExternalLink}
              label="Open in system browser"
              size="sm"
              disabled={!active}
              onClick={() => active && openUrl(active.url).catch((err) => setError(errorMessage(err)))}
            />
          </div>
        </div>

        <div className={active?.loading ? "browser-progress browser-progress-active" : "browser-progress"} aria-hidden="true" />

        <div ref={contentRef} className="browser-content">
          {!active && (
            <EmptyState
              icon={AppWindow}
              size="md"
              title="No tabs open"
              description="Open a tab and type an address or a search. Local sites work too: localhost:3000, myapp.test."
            />
          )}
        </div>
      </div>
    </Page>
  );
}
