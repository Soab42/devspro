import { useEffect, useRef, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import { MODULE_GROUPS, modules } from "./ModuleRegistry";
import { ThemeControl } from "./ThemeControl";
import { useLayoutMode } from "./useLayoutMode";
import { Icon } from "../ui";
import { IconButton } from "../ui";
import { Lock, Menu, PanelLeftClose, PanelLeftOpen, Settings } from "../ui/icons";
// The profile dropdown reuses .menu-item / .menu-separator so both popovers in
// the app look identical. Imported explicitly rather than relying on some other
// module having pulled Menu.css in first.
import "../ui/Menu.css";
import "./Shell.css";

const COLLAPSE_KEY = "devsuite:sidebar-collapsed";

export default function Shell() {
  const { profile, lock } = useAuth();
  const layout = useLayoutMode();
  const location = useLocation();

  const [drawerOpen, setDrawerOpen] = useState(false);
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
        <div className="sidebar-backdrop" onClick={() => setDrawerOpen(false)} />
      )}

      <aside className="sidebar">
        <div className="sidebar-brand">
          <span className="sidebar-mark" aria-hidden="true">
            DP
          </span>
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
            const items = modules.filter((m) => m.group === group.id);
            if (items.length === 0) return null;
            return (
              <div className="sidebar-group" key={group.id}>
                {/* In rail mode the label is hidden and the group's top border
                    carries the grouping instead. */}
                {group.label && <p className="sidebar-group-label">{group.label}</p>}
                {items.map((m) => (
                  <NavLink
                    key={m.id}
                    to={m.path}
                    title={railed ? m.label : undefined}
                    className={({ isActive }) =>
                      `sidebar-link${isActive ? " sidebar-link-active" : ""}`
                    }
                  >
                    <Icon icon={m.icon} size="sm" className="sidebar-link-icon" />
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
          <div className="topbar-spacer" />
          <ThemeControl />
          <ProfileMenu name={profile?.display_name ?? "Profile"} onLock={lock} />
        </header>

        <main className="content">
          <Outlet />
        </main>
      </div>
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
          <button type="button" className="menu-item" role="menuitem" onClick={onLock}>
            <Icon icon={Lock} size="sm" />
            Lock
          </button>
        </div>
      )}
    </div>
  );
}
