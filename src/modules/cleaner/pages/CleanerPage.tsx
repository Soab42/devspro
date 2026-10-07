import { lazy, Suspense, useEffect } from "react";
import { flushOnClose, flushSnapshots, useTabSlot } from "../tabState";
import { useModule } from "../../../app/ModuleRegistry";
import { Page, Spinner, Tabs } from "../../../ui";

const OverviewTab = lazy(() => import("../components/OverviewTab"));
const CachesTab = lazy(() => import("../components/CachesTab"));
const DockerTab = lazy(() => import("../components/DockerTab"));
const LargeFilesTab = lazy(() => import("../components/LargeFilesTab"));
const StorageMapTab = lazy(() => import("../components/StorageMapTab"));
const ProjectsTab = lazy(() => import("../components/ProjectsTab"));

const TABS = [
  { id: "overview", label: "Overview" },
  { id: "caches", label: "Caches" },
  { id: "docker", label: "Docker" },
  { id: "large-files", label: "Large files" },
  { id: "storage-map", label: "Storage map" },
  { id: "projects", label: "Project junk" },
];

export default function CleanerPage() {
  const module = useModule("cleaner");
  // Kept in the tab store so coming back to the Cleaner lands on the same tab.
  const [tab, setTab] = useTabSlot("page", "active", "overview");

  // Saved results are written after a short delay; leaving or closing must not lose them.
  useEffect(() => {
    const off = flushOnClose();
    return () => {
      off();
      void flushSnapshots();
    };
  }, []);

  return (
    <Page
      title={module.label}
      icon={module.icon}
      description="Find what is taking disk space and remove it. Nothing is removed until you have reviewed it."
      tabs={<Tabs tabs={TABS} active={tab} onChange={setTab} ariaLabel="Cleaner sections" />}
    >
      <Suspense fallback={<Spinner />}>
        {tab === "overview" && <OverviewTab />}
        {tab === "caches" && <CachesTab />}
        {tab === "docker" && <DockerTab />}
        {tab === "large-files" && <LargeFilesTab />}
        {tab === "storage-map" && <StorageMapTab />}
        {tab === "projects" && <ProjectsTab />}
      </Suspense>
    </Page>
  );
}
