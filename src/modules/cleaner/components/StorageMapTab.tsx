import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function StorageMapTab() {
  return <EmptyState icon={Eraser} size="md" title="Storage map" description="A size map of a folder and a breakdown by file type. Coming soon." />;
}
