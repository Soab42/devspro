import {
  KeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { useSearchParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useModule } from "../../../app/ModuleRegistry";
import {
  POPOUT_ACTIONS_SLOT,
  currentPopoutModuleId,
  isBrowserTabWindow,
  openBrowserTabWindow,
} from "../../../app/popout";
import { usePopoutHandoff } from "../../../app/popoutState";
import {
  Callout,
  EmptyState,
  Icon,
  IconButton,
  Menu,
  Page,
  SegmentedControl,
  Select,
} from "../../../ui";
import type { MenuItem } from "../../../ui";
import {
  AppWindow,
  ChevronLeft,
  ChevronRight,
  Clock3,
  Code,
  Columns2,
  Compass,
  ExternalLink,
  Flame,
  Globe,
  KeyRound,
  PictureInPicture2,
  Printer,
  RotateCw,
  Search,
  Star,
  Trash2,
  X,
  ZoomIn,
  ZoomOut,
  Bookmark as BookmarkIcon,
} from "../../../ui/icons";
import {
  Bookmark,
  Bounds,
  CREDENTIAL_EVENT,
  Credential,
  CredentialEvent,
  DOWNLOAD_EVENT,
  DownloadEvent,
  LOAD_EVENT,
  LoadEvent,
  NEW_WINDOW_EVENT,
  NewWindowEvent,
  SystemBrowser,
  TITLE_EVENT,
  TitleEvent,
  browserBack,
  browserBookmarkAdd,
  browserBookmarkDelete,
  browserBookmarksList,
  browserClearData,
  browserClose,
  browserCredentialFill,
  browserCredentialSave,
  browserCredentialsFor,
  browserDevtools,
  browserFind,
  browserFocus,
  browserFocusParent,
  browserForward,
  browserNavigate,
  browserOpen,
  browserOpenIn,
  browserPrint,
  browserReload,
  browserSetBounds,
  browserSetVisible,
  browserSetZoom,
  browserStop,
  browserSystemBrowsers,
} from "../api";
import {
  Tab,
  focusedTab,
  getState,
  setState,
  tabById,
  updateTab,
  useBrowserState,
} from "../store";
import {
  DEFAULT_URL,
  Viewport,
  allDevices,
  clampViewport,
  deviceOptionLabel,
  viewportOf,
  Skin,
  deviceById,
  errorMessage,
  nextZoom,
  originOf,
  parseSession,
  serializeSession,
  tabLabel,
  userAgentFor,
} from "../utils";
import { AddressBar } from "../components/AddressBar";
import {
  BookmarksBar,
  DownloadsBar,
  FindBar,
  SavePasswordBar,
} from "../components/Bars";
import { LibraryModal, LibraryTab } from "../components/LibraryModal";
import { DeviceBar, ViewportFrame } from "../components/DeviceBar";
import { TabAction, TabStrip } from "../components/TabStrip";
import "./BrowserPage.css";

const SESSION_KEY = "devspro.browser.session";
const NEVER_KEY = "devspro.browser.neverSave";

function measure(el: HTMLElement | null): Bounds | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) return null;
  return { x: r.left, y: r.top, width: r.width, height: r.height };
}

/**
 * Shown in a pane while its native view is hidden behind a popup. The view
 * paints above the whole DOM, so a menu or dropdown can only be seen if the
 * page steps aside; this says so instead of leaving a blank pane.
 */
function CoveredNotice({ tab }: { tab: Tab }) {
  return (
    <div className="browser-covered" aria-hidden="true">
      <Icon icon={Globe} size="md" />
      <span className="browser-covered-title">
        {tabLabel(tab.title, tab.url)}
      </span>
      <span className="browser-covered-hint">
        Page hidden while a menu is open
      </span>
    </div>
  );
}

function readNever(): Set<string> {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(NEVER_KEY) ?? "[]");
    return new Set(
      Array.isArray(v)
        ? v.filter((x): x is string => typeof x === "string")
        : [],
    );
  } catch {
    return new Set();
  }
}

/**
 * A small browser with three familiar faces and the parts of a real one that
 * matter day to day: persistent sessions, bookmarks, saved logins, history,
 * find, print, the Web Inspector, a split view, device presets and tabs that
 * pop out into their own window.
 *
 * The page content is a native child webview (see modules/browser/mod.rs), so
 * this component only draws the frame. The skin changes the arrangement of
 * that frame *and* the user-agent the tab presents, which is what makes it
 * useful for cross-browser checks; the engine is always WebKitGTK.
 *
 * In a popped-out tab window (`isBrowserTabWindow`) the page runs in "solo"
 * mode: one tab, no strip, no split, and the URL comes from the route.
 */
export default function BrowserPage() {
  const module = useModule("browser");
  const browserState = useBrowserState();
  const {
    tabs,
    activeId,
    splitId,
    focus,
    skin,
    deviceId,
    viewport,
    customDevices,
  } = browserState;
  const [params] = useSearchParams();
  const solo = isBrowserTabWindow();
  // Any popped-out window — the whole module or one tab — drops the page
  // header and fills the window; only the single-tab one also drops the strip.
  const popped = solo || currentPopoutModuleId() === "browser";

  const [error, setError] = useState<string | null>(null);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [logins, setLogins] = useState<Credential[]>([]);
  const [pending, setPending] = useState<CredentialEvent | null>(null);
  const [downloads, setDownloads] = useState<DownloadEvent[]>([]);
  const [systemBrowsers, setSystemBrowsers] = useState<SystemBrowser[]>([]);
  const [findOpen, setFindOpen] = useState(false);
  const [library, setLibrary] = useState<LibraryTab | null>(null);
  // Native views paint above the DOM, so any popup that would overlap the
  // page — the menu, address suggestions, a tab's context menu, the library
  // modal — has to hide them for as long as it is up.
  const [overlays, setOverlays] = useState<Set<string>>(() => new Set());
  const covered = overlays.size > 0 || library !== null;
  const setOverlay = useCallback((name: string, open: boolean) => {
    setOverlays((prev) => {
      if (prev.has(name) === open) return prev;
      const next = new Set(prev);
      if (open) next.add(name);
      else next.delete(name);
      return next;
    });
  }, []);

  const mainRef = useRef<HTMLDivElement>(null);
  const splitRef = useRef<HTMLDivElement>(null);
  // Solo: the toolbar controls move up into the window's top bar, and the
  // window is titled after the page, like a real browser window.
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    if (popped) setSlot(document.getElementById(POPOUT_ACTIONS_SLOT));
  }, [popped]);
  const addressRef = useRef<HTMLInputElement>(null);

  // `main` sits in the left pane and `split` in the right; `active` is
  // whichever the user last clicked, and everything in the toolbar acts on it.
  const main = tabById(activeId);
  const split = tabById(splitId);
  const active = focusedTab(browserState);
  const device = deviceById(deviceId, customDevices);
  const wantedAgent = userAgentFor(skin, device);

  const reloadBookmarks = useCallback(() => {
    browserBookmarksList()
      .then(setBookmarks)
      .catch(() => undefined);
  }, []);

  // ---------------------------------------------------------- native views

  // Every bounds update is a native relayout of the whole window. A window
  // resize or a divider drag fires the observers many times per frame, so
  // the work is coalesced to one pass per animation frame, and a pane whose
  // box has not moved is left alone.
  const lastBounds = useRef(new Map<string, Bounds>());
  const fit = useCallback((id: string | null, el: HTMLElement | null) => {
    if (!id) return;
    const bounds = measure(el);
    if (!bounds) return;
    const prev = lastBounds.current.get(id);
    if (
      prev &&
      prev.x === bounds.x &&
      prev.y === bounds.y &&
      prev.width === bounds.width &&
      prev.height === bounds.height
    )
      return;
    lastBounds.current.set(id, bounds);
    browserSetBounds(id, bounds).catch(() => {
      lastBounds.current.delete(id);
    });
  }, []);

  const fitFrame = useRef<number | null>(null);
  const fitAll = useCallback(() => {
    if (fitFrame.current !== null) return;
    fitFrame.current = requestAnimationFrame(() => {
      fitFrame.current = null;
      const s = getState();
      fit(s.activeId, mainRef.current);
      fit(s.splitId, splitRef.current);
    });
  }, [fit]);
  useEffect(
    () => () => {
      if (fitFrame.current !== null) cancelAnimationFrame(fitFrame.current);
    },
    [],
  );

  const overlayUp = useRef(false);

  /** Show exactly the tabs in the panes; hide every other one (and all of them while a popup is up). */
  const syncVisibility = useCallback(async () => {
    const s = getState();
    const shown = overlayUp.current
      ? new Set<string>()
      : new Set([s.activeId, s.splitId].filter(Boolean) as string[]);
    await Promise.all(
      s.tabs.map((t) =>
        browserSetVisible(t.id, shown.has(t.id)).catch(() => undefined),
      ),
    );
    fitAll();
  }, [fitAll]);

  // One native operation per tab at a time. Opening, closing and the
  // user-agent refresh all touch the same view, and two of them overlapping
  // (a session restore racing the agent check, say) left the native side
  // with "that tab already exists" and the page with nothing.
  const tabOps = useRef(new Map<string, Promise<void>>());
  const withTabLock = useCallback((id: string, fn: () => Promise<void>) => {
    const prev = tabOps.current.get(id) ?? Promise.resolve();
    const next = prev.catch(() => undefined).then(fn);
    tabOps.current.set(id, next);
    return next.finally(() => {
      if (tabOps.current.get(id) === next) tabOps.current.delete(id);
    });
  }, []);

  const createView = useCallback(async (tab: Tab, el: HTMLElement | null) => {
    const bounds = measure(el) ?? { x: 0, y: 0, width: 800, height: 600 };
    const s = getState();
    const ua = userAgentFor(s.skin, deviceById(s.deviceId, s.customDevices));
    // Claim the agent before the view exists, so a refresh that runs in the
    // meantime sees nothing to do.
    updateTab(tab.id, { userAgent: ua });
    lastBounds.current.set(tab.id, bounds);
    await browserOpen(tab.id, tab.url, bounds, ua);
    if (tab.zoom !== 1)
      await browserSetZoom(tab.id, tab.zoom).catch(() => undefined);
  }, []);

  /** Recreate a view whose UA no longer matches the skin/device. Same URL, same session. */
  const ensureAgent = useCallback(
    (tab: Tab, el: HTMLElement | null) =>
      withTabLock(tab.id, async () => {
        const s = getState();
        const ua = userAgentFor(
          s.skin,
          deviceById(s.deviceId, s.customDevices),
        );
        const current = tabById(tab.id);
        if (!current || current.userAgent === ua) return;
        await browserClose(tab.id).catch(() => undefined);
        await createView({ ...current, userAgent: ua }, el);
      }),
    [createView, withTabLock],
  );

  const openTab = useCallback(
    async (url: string = DEFAULT_URL, opts: { background?: boolean } = {}) => {
      const id = crypto.randomUUID();
      const tab: Tab = {
        id,
        url,
        title: "",
        loading: true,
        zoom: 1,
        userAgent: "",
      };
      const previous = getState().activeId;
      setState((s) => ({
        tabs: [...s.tabs, tab],
        activeId: opts.background && previous ? previous : id,
      }));
      try {
        await withTabLock(id, () => createView(tab, mainRef.current));
        await syncVisibility();
      } catch (e) {
        setError(errorMessage(e));
        setState((s) => ({
          tabs: s.tabs.filter((t) => t.id !== id),
          activeId: s.activeId === id ? previous : s.activeId,
        }));
        await syncVisibility();
      }
    },
    [createView, syncVisibility, withTabLock],
  );

  /**
   * A click in the tab strip. A tab already on screen just takes focus; an
   * unseen one replaces the tab in the focused pane, so a split stays a
   * split. The page also gets keyboard focus so typing goes into it.
   */
  const activate = useCallback(
    async (id: string) => {
      const s = getState();
      if (id === s.splitId) {
        setState({ focus: "split" });
      } else if (id === s.activeId) {
        setState({ focus: "main" });
      } else if (s.focus === "split" && s.splitId) {
        setState({ splitId: id });
        const tab = tabById(id);
        if (tab) await ensureAgent(tab, splitRef.current);
      } else {
        setState({ activeId: id, focus: "main" });
        const tab = tabById(id);
        if (tab) await ensureAgent(tab, mainRef.current);
      }
      await syncVisibility();
      await browserFocus(id).catch(() => undefined);
    },
    [ensureAgent, syncVisibility],
  );

  const focusPane = useCallback((pane: "main" | "split") => {
    const s = getState();
    setState({ focus: pane === "split" && s.splitId ? "split" : "main" });
    const id = pane === "split" ? s.splitId : s.activeId;
    if (id) void browserFocus(id).catch(() => undefined);
  }, []);

  const closeTab = useCallback(
    async (id: string) => {
      const s = getState();
      const index = s.tabs.findIndex((t) => t.id === id);
      const remaining = s.tabs.filter((t) => t.id !== id);
      let nextActive = s.activeId;
      if (s.activeId === id)
        nextActive =
          remaining[Math.min(index, remaining.length - 1)]?.id ?? null;
      const splitNext =
        s.splitId === id || s.splitId === nextActive ? null : s.splitId;
      setState({
        tabs: remaining,
        activeId: nextActive,
        splitId: splitNext,
        focus: splitNext ? s.focus : "main",
      });
      await withTabLock(id, () => browserClose(id).catch(() => undefined));
      if (nextActive) {
        const t = tabById(nextActive);
        if (t) await ensureAgent(t, mainRef.current);
      }
      await syncVisibility();
    },
    [ensureAgent, syncVisibility, withTabLock],
  );

  const toggleSplit = useCallback(
    async (id: string | null) => {
      const s = getState();
      if (!id || s.splitId === id) {
        setState({ splitId: null, focus: "main" });
      } else if (id === s.activeId) {
        // Splitting the active tab with itself makes no sense; pick a neighbour.
        const other = s.tabs.find((t) => t.id !== id);
        if (!other) {
          setError("Open a second tab to split the view.");
          return;
        }
        setState({ splitId: other.id });
      } else {
        setState({ splitId: id });
      }
      // The right pane appears on the next paint; measure after it exists.
      await new Promise((r) => requestAnimationFrame(r));
      const t = tabById(getState().splitId);
      if (t) await ensureAgent(t, splitRef.current);
      await syncVisibility();
    },
    [ensureAgent, syncVisibility],
  );

  const popOut = useCallback(
    async (id: string) => {
      const tab = tabById(id);
      if (!tab) return;
      try {
        await openBrowserTabWindow(tab.url, tab.title);
        await closeTab(id);
      } catch (e) {
        setError(errorMessage(e));
      }
    },
    [closeTab],
  );

  // The shell's "new window" button moves the tab you're looking at, the
  // same as the tab's own pop-out; with no tab open it gets a fresh browser.
  usePopoutHandoff(() => ({
    open: async () => {
      const id = focusedTab(getState())?.id ?? getState().activeId;
      if (!id) return false;
      await popOut(id);
      return true;
    },
  }));

  // ---------------------------------------------------------- lifecycle

  // Mount: re-adopt surviving tabs, or restore the last session, or (solo)
  // open the URL the window was created for. Unmount: hide everything so a
  // page never sits on top of another module.
  useEffect(() => {
    reloadBookmarks();
    browserSystemBrowsers()
      .then(setSystemBrowsers)
      .catch(() => undefined);
    const s = getState();
    if (s.tabs.length > 0) {
      void syncVisibility();
    } else if (solo) {
      const url = params.get("url");
      void openTab(url && /^https?:\/\//i.test(url) ? url : DEFAULT_URL);
    } else {
      const saved = parseSession(
        (() => {
          try {
            return localStorage.getItem(SESSION_KEY);
          } catch {
            return null;
          }
        })(),
      );
      if (saved) {
        void (async () => {
          for (const [i, url] of saved.urls.entries())
            await openTab(url, { background: i !== saved.active });
          const target = getState().tabs[saved.active];
          if (target) await activate(target.id);
        })();
      }
    }
    return () => {
      for (const t of getState().tabs)
        browserSetVisible(t.id, false).catch(() => undefined);
    };
    // Mount-only by design; the callbacks read the store directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keyboard focus lives in whichever X window was clicked last (see
  // x11_focus.rs). A click on the app's own UI — this document — hands it
  // back to the app so the address bar and shortcuts work again.
  useEffect(() => {
    const onDown = () => {
      if (getState().tabs.length > 0)
        browserFocusParent().catch(() => undefined);
    };
    document.addEventListener("mousedown", onDown, true);
    return () => document.removeEventListener("mousedown", onDown, true);
  }, []);

  // Session: which URLs are open and which is active. Not in solo windows —
  // those are one-offs and would otherwise overwrite the main window's list.
  useEffect(() => {
    if (solo) return;
    try {
      const ids = tabs.map((t) => t.id);
      localStorage.setItem(
        SESSION_KEY,
        JSON.stringify(serializeSession(tabs, activeId, ids)),
      );
    } catch {
      /* fine */
    }
  }, [tabs, activeId, solo]);

  // Keep the native views glued to their panes through window resizes,
  // sidebar collapse, skin changes and device presets.
  useEffect(() => {
    fitAll();
    const ro = new ResizeObserver(fitAll);
    if (mainRef.current) ro.observe(mainRef.current);
    if (splitRef.current) ro.observe(splitRef.current);
    ro.observe(document.documentElement);
    window.addEventListener("resize", fitAll);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", fitAll);
    };
  }, [
    activeId,
    splitId,
    skin,
    deviceId,
    viewport,
    bookmarks.length,
    findOpen,
    pending,
    downloads.length,
    fitAll,
  ]);

  // A skin or device change means a new UA for the shown tabs, now; the rest
  // catch up when they are next shown.
  useEffect(() => {
    void (async () => {
      const s = getState();
      const a = tabById(s.activeId);
      const b = tabById(s.splitId);
      if (a) await ensureAgent(a, mainRef.current);
      if (b) await ensureAgent(b, splitRef.current);
      await syncVisibility();
    })();
  }, [wantedAgent, ensureAgent, syncVisibility]);

  useEffect(() => {
    overlayUp.current = overlays.size > 0 || library !== null;
    void syncVisibility();
  }, [overlays, library, syncVisibility]);

  // Saved logins for the active site, for the key icon in the address bar.
  const activeOrigin = active ? originOf(active.url) : null;
  useEffect(() => {
    if (!activeOrigin) {
      setLogins([]);
      return;
    }
    browserCredentialsFor(activeOrigin)
      .then(setLogins)
      .catch(() => setLogins([]));
  }, [activeOrigin, pending]);

  useEffect(() => {
    const unlisteners = [
      listen<TitleEvent>(TITLE_EVENT, (e) =>
        updateTab(e.payload.id, { title: e.payload.title }),
      ),
      listen<LoadEvent>(LOAD_EVENT, (e) => {
        const { id, url, state } = e.payload;
        updateTab(id, { url, loading: state === "started" });
      }),
      listen<NewWindowEvent>(
        NEW_WINDOW_EVENT,
        (e) => void openTab(e.payload.url),
      ),
      listen<DownloadEvent>(DOWNLOAD_EVENT, (e) => {
        setDownloads((ds) => {
          const key = e.payload.url;
          const rest = ds.filter((d) => d.url !== key);
          return [e.payload, ...rest].slice(0, 5);
        });
      }),
      listen<CredentialEvent>(CREDENTIAL_EVENT, (e) => {
        if (readNever().has(e.payload.origin)) return;
        setPending(e.payload);
      }),
    ];
    return () => {
      for (const p of unlisteners) p.then((un) => un()).catch(() => undefined);
    };
    // Subscribed once; re-subscribing on every render would race with events
    // fired between unlisten and listen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------------------------------------------------------- actions

  const navigate = useCallback(
    (url: string) => {
      const a = focusedTab();
      if (a) {
        updateTab(a.id, { loading: true });
        browserNavigate(a.id, url).catch((err) => setError(errorMessage(err)));
      } else {
        void openTab(url);
      }
    },
    [openTab],
  );

  const bookmarkOf = (url: string | undefined) =>
    url ? bookmarks.find((b) => b.url === url) : undefined;

  async function toggleBookmark(tab: Tab | null = active) {
    if (!tab) return;
    try {
      const existing = bookmarkOf(tab.url);
      if (existing) await browserBookmarkDelete(existing.id);
      else await browserBookmarkAdd(tab.url, tabLabel(tab.title, tab.url));
      reloadBookmarks();
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  async function savePending() {
    if (!pending) return;
    try {
      await browserCredentialSave(
        pending.origin,
        pending.username,
        pending.password,
      );
      setPending(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  function neverForSite() {
    if (!pending) return;
    try {
      const set = readNever();
      set.add(pending.origin);
      localStorage.setItem(NEVER_KEY, JSON.stringify([...set]));
    } catch {
      /* fine */
    }
    setPending(null);
  }

  async function zoom(direction: -1 | 1) {
    if (!active) return;
    const factor = nextZoom(active.zoom, direction);
    await browserSetZoom(active.id, factor).catch((err) =>
      setError(errorMessage(err)),
    );
    updateTab(active.id, { zoom: factor });
  }

  function onTabAction(id: string, action: TabAction) {
    const tab = tabById(id);
    if (!tab) return;
    switch (action) {
      case "duplicate":
        void openTab(tab.url);
        break;
      case "popout":
        void popOut(id);
        break;
      case "split":
        void toggleSplit(id);
        break;
      case "bookmark":
        void toggleBookmark(tab);
        break;
      case "close":
        void closeTab(id);
        break;
      case "close-others":
        for (const t of getState().tabs) if (t.id !== id) void closeTab(t.id);
        void activate(id);
        break;
    }
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === "F12") {
      e.preventDefault();
      if (active) void browserDevtools(active.id, true);
      return;
    }
    if (e.key === "Escape" && findOpen) {
      setFindOpen(false);
      return;
    }
    if (!(e.ctrlKey || e.metaKey)) return;
    const key = e.key.toLowerCase();
    if (key === "l") {
      e.preventDefault();
      addressRef.current?.focus();
      addressRef.current?.select();
    } else if (key === "t" && !solo) {
      e.preventDefault();
      void openTab();
    } else if (key === "w" && active) {
      e.preventDefault();
      void closeTab(active.id);
    } else if (key === "f" && active) {
      e.preventDefault();
      setFindOpen(true);
    } else if (key === "d" && active) {
      e.preventDefault();
      void toggleBookmark();
    } else if (key === "p" && active) {
      e.preventDefault();
      void browserPrint(active.id);
    } else if (key === "i" && e.shiftKey && active) {
      e.preventDefault();
      void browserDevtools(active.id, true);
    } else if (key === "\\" && !solo) {
      e.preventDefault();
      void toggleSplit(splitId ?? activeId);
    } else if (key === "r" && active) {
      e.preventDefault();
      void browserReload(active.id);
    } else if (key === "=" || key === "+") {
      e.preventDefault();
      void zoom(1);
    } else if (key === "-") {
      e.preventDefault();
      void zoom(-1);
    }
  }

  // ---------------------------------------------------------- menu

  const menuItems = useMemo<MenuItem[]>(() => {
    const items: MenuItem[] = [];
    if (!solo)
      items.push({
        key: "new",
        label: "New tab",
        icon: AppWindow,
        onClick: () => void openTab(),
      });
    if (active && !solo) {
      items.push({
        key: "popout",
        label: "Pop out this tab",
        icon: PictureInPicture2,
        onClick: () => void popOut(active.id),
      });
      items.push({
        key: "split",
        label: splitId ? "Leave split view" : "Split view",
        icon: Columns2,
        onClick: () => void toggleSplit(splitId ?? activeId),
      });
    }
    if (active) {
      items.push({
        key: "find",
        label: "Find in page",
        icon: Search,
        onClick: () => setFindOpen(true),
      });
      items.push({
        key: "inspect",
        label: "Inspect (Web Inspector)",
        icon: Code,
        onClick: () => void browserDevtools(active.id, true),
      });
      items.push({
        key: "print",
        label: "Print…",
        icon: Printer,
        onClick: () => void browserPrint(active.id),
      });
      items.push({
        key: "bookmark",
        label: bookmarkOf(active.url)
          ? "Remove bookmark"
          : "Bookmark this page",
        icon: Star,
        onClick: () => void toggleBookmark(),
      });
    }
    items.push({
      key: "bookmarks",
      label: "Bookmarks",
      icon: BookmarkIcon,
      separatorBefore: true,
      onClick: () => setLibrary("bookmarks"),
    });
    items.push({
      key: "passwords",
      label: "Passwords",
      icon: KeyRound,
      onClick: () => setLibrary("passwords"),
    });
    items.push({
      key: "history",
      label: "History",
      icon: Clock3,
      onClick: () => setLibrary("history"),
    });
    if (active) {
      items.push({
        key: "system",
        label: "Open in system browser",
        icon: ExternalLink,
        separatorBefore: true,
        onClick: () =>
          void openUrl(active.url).catch((err) => setError(errorMessage(err))),
      });
      for (const b of systemBrowsers) {
        items.push({
          key: `open-${b.id}`,
          label: `Open in ${b.label}`,
          icon: Globe,
          onClick: () =>
            void browserOpenIn(b.id, active.url).catch((err) =>
              setError(errorMessage(err)),
            ),
        });
      }
    }
    items.push({
      key: "clear",
      label: "Clear browsing data",
      icon: Trash2,
      danger: true,
      separatorBefore: true,
      disabled: tabs.length === 0,
      onClick: () =>
        void browserClearData().catch((err) => setError(errorMessage(err))),
    });
    return items;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, activeId, splitId, solo, tabs.length, bookmarks, systemBrowsers]);

  // ---------------------------------------------------------- render

  // The pinned size boxes the viewport; the native view takes whatever the
  // box measures, so clamping to the pane here is the whole fit. While a
  // handle is being dragged the box shows its size and the view is hidden.
  const [resizing, setResizing] = useState(false);
  // Each pane's inner size, so a pinned viewport can be sized in plain
  // pixels: percentages don't resolve inside the resizable frame, and an
  // unresolved one collapsed the page to a 1×1 widget.
  const mainPaneRef = useRef<HTMLDivElement>(null);
  const splitPaneRef = useRef<HTMLDivElement>(null);
  const [paneSizes, setPaneSizes] = useState({
    main: { w: 0, h: 0 },
    split: { w: 0, h: 0 },
  });
  useEffect(() => {
    const read = () => {
      const m = mainPaneRef.current?.getBoundingClientRect();
      const sp = splitPaneRef.current?.getBoundingClientRect();
      const next = {
        main: { w: Math.round(m?.width ?? 0), h: Math.round(m?.height ?? 0) },
        split: {
          w: Math.round(sp?.width ?? 0),
          h: Math.round(sp?.height ?? 0),
        },
      };
      setPaneSizes((prev) =>
        prev.main.w === next.main.w &&
        prev.main.h === next.main.h &&
        prev.split.w === next.split.w &&
        prev.split.h === next.split.h
          ? prev
          : next,
      );
    };
    read();
    const ro = new ResizeObserver(read);
    if (mainPaneRef.current) ro.observe(mainPaneRef.current);
    if (splitPaneRef.current) ro.observe(splitPaneRef.current);
    return () => ro.disconnect();
  }, [splitId, activeId]);

  const PANE_GUTTER = 24;
  const styleFor = (size: { w: number; h: number }) => {
    if (!viewport) return undefined;
    if (size.w === 0) return { width: viewport.width, height: viewport.height };
    return {
      width: Math.max(1, Math.min(viewport.width, size.w - PANE_GUTTER)),
      height: Math.max(1, Math.min(viewport.height, size.h - PANE_GUTTER)),
    };
  };
  const viewportClass = [
    "browser-viewport",
    viewport && "browser-viewport-device",
    resizing && "browser-viewport-resizing",
  ]
    .filter(Boolean)
    .join(" ");
  const sizeLabel = viewport
    ? `${viewport.width} × ${viewport.height}`
    : undefined;

  function chooseDevice(id: string) {
    const d = deviceById(id, getState().customDevices);
    setState({ deviceId: d.id, viewport: viewportOf(d) });
  }

  function setViewport(v: Viewport) {
    setState({ viewport: clampViewport(v) });
  }

  function savePreset(label: string) {
    const v = getState().viewport;
    if (!v) return;
    const preset = {
      id: `custom-${crypto.randomUUID()}`,
      label,
      width: v.width,
      height: v.height,
    };
    setState((s) => ({
      customDevices: [...s.customDevices, preset],
      deviceId: preset.id,
    }));
  }

  function deletePreset(id: string) {
    setState((s) => ({
      customDevices: s.customDevices.filter((c) => c.id !== id),
      deviceId: s.deviceId === id ? "responsive" : s.deviceId,
    }));
  }

  const paneMax = (size: { w: number; h: number }) => () => ({
    width: Math.max(200, (size.w || 800) - PANE_GUTTER),
    height: Math.max(200, (size.h || 600) - PANE_GUTTER),
  });

  const framed = (size: { w: number; h: number }, node: React.ReactNode) =>
    viewport ? (
      <ViewportFrame
        viewport={viewport}
        maxSize={paneMax(size)}
        onResizeStart={() => {
          setResizing(true);
          setOverlay("resize", true);
        }}
        onResize={setViewport}
        onResizeEnd={() => {
          setResizing(false);
          setOverlay("resize", false);
        }}
      >
        {node}
      </ViewportFrame>
    ) : (
      node
    );

  const bookmarked = !!bookmarkOf(active?.url);

  useEffect(() => {
    if (!popped) return;
    document.title = active
      ? `${tabLabel(active.title, active.url)} \u2014 DevsPro`
      : "Browser \u2014 DevsPro";
  }, [popped, active]);

  const actions = (
    <div className="browser-actions">
      <Select
        size="sm"
        aria-label="Device"
        searchable
        searchPlaceholder="Find a device…"
        menuWidth={300}
        value={deviceId}
        onChange={chooseDevice}
        onOpenChange={(o) => setOverlay("device", o)}
        options={allDevices(customDevices).map((d) => ({
          value: d.id,
          label: deviceOptionLabel(d),
        }))}
      />
      <SegmentedControl
        ariaLabel="Browser identity"
        size="sm"
        value={skin}
        onChange={(v: Skin) => setState({ skin: v })}
        options={[
          { value: "chrome", label: "Chrome", icon: Globe },
          { value: "safari", label: "Safari", icon: Compass },
          { value: "firefox", label: "Firefox", icon: Flame },
        ]}
      />
    </div>
  );

  const body = (
    <>
      {error && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error}
        </Callout>
      )}
      <div
        className={`browser browser-skin-${skin}${popped ? " browser-popped" : ""}${solo ? " browser-solo" : ""}`}
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        {!solo && (
          <TabStrip
            tabs={tabs}
            activeId={active?.id ?? null}
            splitId={splitId}
            onActivate={(id) => void activate(id)}
            onClose={(id) => void closeTab(id)}
            onNew={() => void openTab()}
            onAction={onTabAction}
            onOverlay={(o) => setOverlay("tabs", o)}
          />
        )}

        <div className="browser-nav">
          <div className="browser-nav-group">
            <IconButton
              icon={ChevronLeft}
              label="Back"
              size="sm"
              disabled={!active}
              onClick={() => active && browserBack(active.id)}
            />
            <IconButton
              icon={ChevronRight}
              label="Forward"
              size="sm"
              disabled={!active}
              onClick={() => active && browserForward(active.id)}
            />
            <IconButton
              icon={active?.loading ? X : RotateCw}
              label={active?.loading ? "Stop" : "Reload"}
              size="sm"
              disabled={!active}
              onClick={() =>
                active &&
                (active.loading
                  ? browserStop(active.id)
                  : browserReload(active.id))
              }
            />
          </div>
          <AddressBar
            url={active?.url ?? ""}
            skin={skin}
            disabled={false}
            bookmarks={bookmarks}
            bookmarked={bookmarked}
            logins={logins}
            onNavigate={navigate}
            onToggleBookmark={() => void toggleBookmark()}
            onFill={(credId) =>
              active &&
              browserCredentialFill(active.id, credId).catch((err) =>
                setError(errorMessage(err)),
              )
            }
            onOverlay={(o) => setOverlay("address", o)}
            inputRef={addressRef}
          />
          <div className="browser-nav-group">
            <IconButton
              icon={ZoomOut}
              label="Zoom out"
              size="sm"
              disabled={!active}
              onClick={() => void zoom(-1)}
            />
            {active && active.zoom !== 1 && (
              <span className="browser-zoom u-tnum">
                {Math.round(active.zoom * 100)}%
              </span>
            )}
            <IconButton
              icon={ZoomIn}
              label="Zoom in"
              size="sm"
              disabled={!active}
              onClick={() => void zoom(1)}
            />
            <IconButton
              icon={Code}
              label="Inspect"
              size="sm"
              disabled={!active}
              onClick={() => active && browserDevtools(active.id, true)}
            />
            {!solo && (
              <IconButton
                icon={Columns2}
                label={splitId ? "Leave split view" : "Split view"}
                size="sm"
                disabled={tabs.length < 2 && !splitId}
                onClick={() => void toggleSplit(splitId ?? activeId)}
              />
            )}
            {!solo && (
              <IconButton
                icon={PictureInPicture2}
                label="Pop out this tab"
                size="sm"
                disabled={!active}
                onClick={() => active && void popOut(active.id)}
              />
            )}
            <Menu
              items={menuItems}
              label="Browser menu"
              size="sm"
              onOpenChange={(o) => setOverlay("menu", o)}
            />
          </div>
        </div>

        <BookmarksBar
          bookmarks={bookmarks}
          onOpen={(url, newTab) => (newTab ? void openTab(url) : navigate(url))}
        />

        {viewport && (
          <DeviceBar
            device={device}
            viewport={viewport}
            customDevices={customDevices}
            onViewport={setViewport}
            onSavePreset={savePreset}
            onDeletePreset={deletePreset}
            onFullSize={() => chooseDevice("responsive")}
          />
        )}

        <div
          className={
            active?.loading
              ? "browser-progress browser-progress-active"
              : "browser-progress"
          }
          aria-hidden="true"
        />

        {pending && (
          <SavePasswordBar
            pending={pending}
            onSave={() => void savePending()}
            onNever={neverForSite}
            onDismiss={() => setPending(null)}
          />
        )}
        {findOpen && active && (
          <FindBar
            onFind={(text, backwards) =>
              void browserFind(active.id, text, backwards).catch(
                () => undefined,
              )
            }
            onClose={() => setFindOpen(false)}
          />
        )}
        <DownloadsBar
          downloads={downloads}
          onReveal={(path) =>
            void revealItemInDir(path).catch((err) =>
              setError(errorMessage(err)),
            )
          }
          onClear={() => setDownloads([])}
        />

        <div
          className={
            split ? "browser-content browser-content-split" : "browser-content"
          }
        >
          <div
            className={
              split && focus === "main"
                ? "browser-pane browser-pane-focused"
                : "browser-pane"
            }
            onPointerDownCapture={() => split && focusPane("main")}
          >
            {split && (
              <div className="browser-pane-header">
                <Icon icon={Globe} size="xs" />
                <span className="browser-pane-title">
                  {main ? tabLabel(main.title, main.url) : ""}
                </span>
              </div>
            )}
            <div className="browser-pane-body" ref={mainPaneRef}>
              {framed(
                paneSizes.main,
                <div
                  ref={mainRef}
                  className={viewportClass}
                  style={styleFor(paneSizes.main)}
                  data-size={sizeLabel}
                />,
              )}
              {!main && (
                <div className="browser-empty">
                  <EmptyState
                    icon={AppWindow}
                    size="md"
                    title="No tabs open"
                    description="Open a tab and type an address or a search. Local sites work too: localhost:3000, myapp.test."
                  />
                </div>
              )}
              {main && covered && !resizing && <CoveredNotice tab={main} />}
            </div>
          </div>
          {split && (
            <div
              className={
                focus === "split"
                  ? "browser-pane browser-pane-secondary browser-pane-focused"
                  : "browser-pane browser-pane-secondary"
              }
              onPointerDownCapture={() => focusPane("split")}
            >
              <div className="browser-pane-header">
                <Icon icon={Columns2} size="xs" />
                <span className="browser-pane-title">
                  {tabLabel(split.title, split.url)}
                </span>
                <IconButton
                  icon={ChevronLeft}
                  label="Make this the main tab"
                  size="sm"
                  onClick={() => void activate(split.id)}
                />
                <IconButton
                  icon={X}
                  label="Close split"
                  size="sm"
                  onClick={() => void toggleSplit(null)}
                />
              </div>
              <div className="browser-pane-body" ref={splitPaneRef}>
                {framed(
                  paneSizes.split,
                  <div
                    ref={splitRef}
                    className={viewportClass}
                    style={styleFor(paneSizes.split)}
                    data-size={sizeLabel}
                  />,
                )}
                {covered && !resizing && <CoveredNotice tab={split} />}
              </div>
            </div>
          )}
        </div>
      </div>

      {library && (
        <LibraryModal
          initial={library}
          bookmarks={bookmarks}
          onBookmarksChanged={reloadBookmarks}
          onOpen={(url) => {
            setLibrary(null);
            navigate(url);
          }}
          onClose={() => setLibrary(null)}
        />
      )}
    </>
  );

  if (popped) {
    return (
      <>
        {slot && createPortal(actions, slot)}
        {body}
      </>
    );
  }

  return (
    <Page title={module.label} icon={module.icon} actions={actions}>
      {body}
    </Page>
  );
}
