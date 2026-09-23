import { Page, statusOf, useResource } from "../../../ui";
import { useModule } from "../../../app/ModuleRegistry";
import BrowserPanel from "../browser/BrowserPanel";
import { listDatabases } from "../api";
import { isBrowsable } from "../utils";

/**
 * The data browser, as a module in its own right.
 *
 * It was a tab on the Databases page, which put a three-pane workspace inside
 * a page whose other tab is a short table — the browser never got the window
 * it needs, and "manage the containers DevsPro runs" and "read and edit rows
 * in any database" turned out not to be two views of one thing. They also
 * have different scopes: Databases is about DevsPro's own compose project,
 * while this opens anything a saved connection can reach.
 *
 * It still shares the Databases module's code — the two are about the same
 * servers, and splitting the directory would only put the connection handling
 * further from the module that also uses it.
 */
export default function DataBrowserPage() {
  const module = useModule("data-browser");
  const databases = useResource(listDatabases);

  // DevsPro's own MySQL/Postgres services, offered alongside saved custom
  // connections in the picker. Grouped here rather than in BrowserPanel
  // because only this side knows how compose names its container pairs.
  const managed = (databases.data ?? [])
    .filter((c) => !c.name.endsWith("_ui") && isBrowsable(c.image))
    .map((c) => ({
      base: c.name.replace(/^devspro_/, ""),
      running: c.state === "running",
    }));

  return (
    <Page
      title={module.label}
      icon={module.icon}
      status={statusOf(databases)}
      fill
    >
      <BrowserPanel managed={managed} />
    </Page>
  );
}
