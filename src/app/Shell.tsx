import {
  MouseEvent as ReactMouseEvent,
  Suspense,
  useEffect,
  useRef,
  useState,
} from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import {
  MODULE_GROUPS,
  moduleByPath,
  type ModuleDefinition,
} from "./ModuleRegistry";
import { useModules } from "./ModulesContext";
import { ThemeControl } from "./ThemeControl";
import { UpdateIndicator } from "./UpdateIndicator";
import { useLayoutMode } from "./useLayoutMode";
import { CommandPalette } from "./CommandPalette";
import { TerminalDock } from "./TerminalDock";
import { useTerminalDock } from "./terminalDock";
import {
  POPOUT_ACTIONS_SLOT,
  POPOUT_TABS_SLOT,
  currentPopoutModuleId,
  isBrowserTabWindow,
  openModuleWindow,
} from "./popout";
import { currentPopoutHandoff } from "./popoutState";
import { Icon, IconButton, PageChromeContext, Spinner } from "../ui";
import {
  Lock,
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRight,
  Search,
  Settings,
  SquareArrowOutUpRight,
} from "../ui/icons";
// The profile dropdown reuses .menu-item / .menu-separator so both popovers in
// the app look identical. Imported explicitly rather than relying on some other
// module having pulled Menu.css in first.
import "../ui/Menu.css";
import "./Shell.css";

const COLLAPSE_KEY = "devspro:sidebar-collapsed";

const isMac =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform);
/** Rendered in the launcher trigger; the handler accepts both modifiers regardless. */
const PALETTE_SHORTCUT = isMac ? "\u2318K" : "Ctrl K";

/**
 * Opens a module in its own window instead of navigating. Ctrl/Cmd+click and
 * middle-click, the same gestures that open a link in a new tab in a browser.
 */
function wantsNewWindow(e: ReactMouseEvent): boolean {
  return e.button === 1 || e.ctrlKey || e.metaKey;
}

export default function Shell() {
  // A popped-out window shows one module and nothing else: no sidebar, no
  // profile, no launcher. It reads its module from the window label, so the
  // same bundle serves both kinds of window.
  const popoutId = currentPopoutModuleId();
  if (popoutId) return <PopoutShell />;
  return <MainShell />;
}

function PageBoundary() {
  return (
    <Suspense
      fallback={
        <div className="content-loading">
          <Spinner size="md" />
        </div>
      }
    >
      <Outlet />
    </Suspense>
  );
}

function PopoutShell() {
  const location = useLocation();
  const current = moduleByPath(location.pathname);
  // A popped-out browser — the whole module or a single tab — is a browser
  // window of its own: no page header, no padding, and the page sets the
  // window title from the site's title.
  const chromeless = isBrowserTabWindow() || current?.id === "browser";
  // The page's meta and actions move into the top bar; the slot is handed
  // down through context once it exists.
  const [actionsSlot, setActionsSlot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (chromeless) return;
    document.title = current ? `${current.label} \u2014 DevsPro` : "DevsPro";
  }, [current, chromeless]);

  return (
    <div
      className={
        chromeless
          ? "shell shell-popout shell-popout-chromeless"
          : "shell shell-popout"
      }
    >
      <div className="shell-main">
        <header className="topbar">
          {current && (
            <span className="topbar-title">
              <Icon icon={current.icon} size="sm" />
              {current.label}
            </span>
          )}
          <div id={POPOUT_TABS_SLOT} className="topbar-tabs" />
          <div className="topbar-spacer" />
          <div
            id={POPOUT_ACTIONS_SLOT}
            ref={setActionsSlot}
            className="topbar-actions"
          />
          <ThemeControl />
        </header>
        <main className="content">
          <PageChromeContext.Provider value={{ actionsSlot }}>
            <PageBoundary />
          </PageChromeContext.Provider>
        </main>
      </div>
    </div>
  );
}

function MainShell() {
  const { profile, lock } = useAuth();
  const layout = useLayoutMode();
  const location = useLocation();
  const { enabled } = useModules();
  const dock = useTerminalDock();
  const current = moduleByPath(location.pathname);

  const [drawerOpen, setDrawerOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [pinnedCollapsed, setPinnedCollapsed] = useState(
    () => localStorage.getItem(COLLAPSE_KEY) === "1",
  );

  // The user's pin only applies where there is a choice; at "rail" the width
  // decides, and at "drawer" the sidebar is overlaid regardless.
  const railed = layout === "rail" || (layout === "full" && pinnedCollapsed);
  const isDrawer = layout === "drawer";

  useEffect(() => {
    localStorage.setItem(COLLAPSE_KEY, pinnedCollapsed ? "1" : "0");
  }, [pinnedCollapsed]);

  // Navigating should dismiss the drawer, otherwise it stays over the page the
  // user just asked for.
  useEffect(() => setDrawerOpen(false), [location.pathname]);

  useEffect(() => {
    if (!isDrawer) setDrawerOpen(false);
  }, [isDrawer]);

  useEffect(() => {
    if (!drawerOpen) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setDrawerOpen(false);
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [drawerOpen]);

  // The launcher is reachable from anywhere, including from inside a text
  // field — that is the point of a launcher. Ctrl+K and Ctrl+P both work
  // because half the tools people come from use one and half the other.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key !== "k" && key !== "p") return;
      e.preventDefault();
      setPaletteOpen((v) => !v);
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  /** A fresh window for `m`: the sidebar's Ctrl+click and the launcher. */
  function popOut(m: ModuleDefinition) {
    openModuleWindow(m).catch((err) => console.error(err));
  }

  /**
   * The top-bar button: take the page the user is on with them. The page
   * either moves what it has (a terminal pane, a browser tab) or hands over
   * the parameters that recreate it; a page with nothing to carry gets a
   * fresh window like any other.
   */
  async function popOutCurrent(m: ModuleDefinition) {
    try {
      const handoff = currentPopoutHandoff();
      if (handoff?.open && (await handoff.open())) return;
      await openModuleWindow(m, handoff?.params);
    } catch (err) {
      console.error(err);
    }
  }

  function onLinkClick(
    e: ReactMouseEvent<HTMLAnchorElement>,
    m: ModuleDefinition,
  ) {
    if (!wantsNewWindow(e)) return;
    e.preventDefault();
    popOut(m);
  }

  const classes = [
    "shell",
    railed && !isDrawer && "shell-railed",
    // A rail the window forced on us, as opposed to one the user pinned. Only
    // the forced kind hides the expand control, because at this width there is
    // nothing to expand to.
    layout === "rail" && "shell-rail-forced",
    isDrawer && "shell-drawer",
    drawerOpen && "shell-drawer-open",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes}>
      {isDrawer && drawerOpen && (
        <div
          className="sidebar-backdrop"
          onClick={() => setDrawerOpen(false)}
        />
      )}

      <aside className="sidebar">
        <div className="sidebar-brand">
          <img
            className="sidebar-mark"
            src="/logo.svg"
            alt=""
            aria-hidden="true"
          />
          <span className="sidebar-wordmark">DevsPro</span>
          {layout === "full" && (
            <IconButton
              icon={pinnedCollapsed ? PanelLeftOpen : PanelLeftClose}
              label={pinnedCollapsed ? "Expand sidebar" : "Collapse sidebar"}
              size="sm"
              variant="ghost"
              className="sidebar-collapse"
              onClick={() => setPinnedCollapsed((v) => !v)}
            />
          )}
        </div>

        {/* The label belongs on the nav landmark itself, not the <aside>. */}
        <nav className="sidebar-nav" aria-label="Main navigation">
          {MODULE_GROUPS.map((group) => {
            const items = enabled.filter((m) => m.group === group.id);
            if (items.length === 0) return null;
            return (
              <div className="sidebar-group" key={group.id}>
                {/* In rail mode the label is hidden and the group's top border
                    carries the grouping instead. */}
                {group.label && (
                  <p className="sidebar-group-label">{group.label}</p>
                )}
                {items.map((m) => (
                  <NavLink
                    key={m.id}
                    to={m.path}
                    title={railed ? m.label : undefined}
                    onClick={(e) => onLinkClick(e, m)}
                    onAuxClick={(e) => onLinkClick(e, m)}
                    className={({ isActive }) =>
                      `sidebar-link${isActive ? " sidebar-link-active" : ""}`
                    }
                  >
                    <Icon
                      icon={m.icon}
                      size="sm"
                      className="sidebar-link-icon"
                    />
                    <span className="sidebar-link-label">{m.label}</span>
                  </NavLink>
                ))}
              </div>
            );
          })}
        </nav>
      </aside>

      <div className="shell-main">
        <header className="topbar">
          {isDrawer && (
            <IconButton
              icon={Menu}
              label="Toggle navigation"
              variant="ghost"
              onClick={() => setDrawerOpen((v) => !v)}
            />
          )}
          <button
            type="button"
            className="topbar-search"
            onClick={() => setPaletteOpen(true)}
            aria-label="Search modules"
            aria-keyshortcuts="Control+K Meta+K"
          >
            <Icon icon={Search} size="sm" />
            <span className="topbar-search-text">Search modules</span>
            <kbd className="topbar-search-kbd" aria-hidden="true">
              {PALETTE_SHORTCUT}
            </kbd>
          </button>
          <div className="topbar-spacer" />
          <UpdateIndicator />
          {current && (
            <IconButton
              icon={SquareArrowOutUpRight}
              label={`Open ${current.label} in a new window`}
              variant="ghost"
              onClick={() => void popOutCurrent(current)}
            />
          )}
          <IconButton
            icon={PanelRight}
            label={dock.open ? "Hide terminal" : "Show terminal"}
            aria-keyshortcuts="Control+` Meta+`"
            variant="ghost"
            aria-pressed={dock.open}
            onClick={dock.toggle}
          />
          <ThemeControl />
          <ProfileMenu
            name={profile?.display_name ?? "Profile"}
            onLock={lock}
          />
        </header>

        <main className="content">
          <PageBoundary />
        </main>
      </div>

      <TerminalDock
        open={dock.open}
        width={dock.width}
        onWidthChange={dock.setWidth}
        onClose={() => dock.setOpen(false)}
        overlay={isDrawer}
      />

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
      />
    </div>
  );
}

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

function ProfileMenu({ name, onLock }: { name: string; onLock: () => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(e: MouseEvent) {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="profile" ref={ref}>
      <button
        type="button"
        className="profile-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="profile-avatar" aria-hidden="true">
          {initials(name)}
        </span>
        <span className="profile-name">{name}</span>
      </button>

      {open && (
        <div className="profile-menu" role="menu">
          <div className="profile-menu-header">
            <span className="profile-avatar" aria-hidden="true">
              {initials(name)}
            </span>
            <span className="profile-menu-name">{name}</span>
          </div>
          <NavLink to="/settings" className="menu-item" role="menuitem">
            <Icon icon={Settings} size="sm" />
            Settings
          </NavLink>
          <div className="menu-separator" role="separator" />
          {/* Labelled "Lock", not "Log out" — the handler is lock(), which
              returns to the lock screen rather than ending a session. */}
          <button
            type="button"
            className="menu-item"
            role="menuitem"
            onClick={onLock}
          >
            <Icon icon={Lock} size="sm" />
            Lock
          </button>
        </div>
      )}
    </div>
  );
}
