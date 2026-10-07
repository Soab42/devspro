import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function LargeFilesTab() {
  return <EmptyState icon={Eraser} size="md" title="Large files" description="The biggest files on disk, and a folder-by-folder size view. Coming soon." />;
}
