import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { HOME_MODULE, modules, type ModuleDefinition } from "./ModuleRegistry";
import { ModulesProvider, useModules } from "./ModulesContext";
import Shell from "./Shell";

// HashRouter, not BrowserRouter: the packaged app serves the frontend from the
// filesystem via Tauri's asset protocol, not a real HTTP server with history-API
// fallback support.
export default function AppRouter() {
  return (
    <ModulesProvider>
      <HashRouter>
        <Routes>
          <Route element={<Shell />}>
            {modules.map((m) => (
              <Route key={m.id} path={m.path} element={<ModuleRoute module={m} />} />
            ))}
            <Route path="*" element={<Navigate to={HOME_MODULE.path} replace />} />
          </Route>
        </Routes>
      </HashRouter>
    </ModulesProvider>
  );
}

/**
 * A switched-off module's route still exists (deep links, the back button) but
 * lands on the home module instead of rendering a page the sidebar no longer
 * offers. Pages are lazy(); the Suspense boundary is in the shell.
 */
function ModuleRoute({ module }: { module: ModuleDefinition }) {
  const { isEnabled } = useModules();
  if (!isEnabled(module.id)) return <Navigate to={HOME_MODULE.path} replace />;
  return <module.Component />;
}
