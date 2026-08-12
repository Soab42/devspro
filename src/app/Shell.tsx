import { useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { useAuth } from "../auth/AuthContext";
import { modules } from "./ModuleRegistry";
import { useTheme } from "./ThemeContext";
import "./Shell.css";

export default function Shell() {
  const { profile, lock } = useAuth();
  const { theme, setTheme } = useTheme();
  const [sidebarOpen, setSidebarOpen] = useState(false);

  return (
    <div className={`shell${sidebarOpen ? " sidebar-open" : ""}`}>
      <div className="sidebar-backdrop" onClick={() => setSidebarOpen(false)} />
      <aside className="sidebar">
        <div className="sidebar-brand">DevSuite</div>
        <nav>
          {modules.map((m) => (
            <NavLink
              key={m.id}
              to={m.path}
              onClick={() => setSidebarOpen(false)}
              className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`}
            >
              <span className="sidebar-icon">{m.icon}</span>
              {m.label}
            </NavLink>
          ))}
        </nav>
      </aside>
      <div className="shell-main">
        <header className="topbar">
          <button
            className="ghost-button menu-toggle"
            onClick={() => setSidebarOpen((v) => !v)}
            aria-label="Toggle menu"
          >
            ☰
          </button>
          <div className="topbar-spacer" />
          <div className="theme-toggle">
            <button
              className={theme === "light" ? "active" : ""}
              onClick={() => setTheme("light")}
              aria-label="Light theme"
              title="Light"
            >
              ☀
            </button>
            <button
              className={theme === "system" ? "active" : ""}
              onClick={() => setTheme("system")}
              aria-label="System theme"
              title="System"
            >
              ◐
            </button>
            <button
              className={theme === "dark" ? "active" : ""}
              onClick={() => setTheme("dark")}
              aria-label="Dark theme"
              title="Dark"
            >
              ☾
            </button>
          </div>
          <div className="topbar-profile">
            <span>{profile?.display_name}</span>
            <button className="secondary" onClick={lock}>
              Log out
            </button>
          </div>
        </header>
        <main className="content">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
