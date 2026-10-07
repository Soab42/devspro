import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function CachesTab() {
  return <EmptyState icon={Eraser} size="md" title="Caches" description="Package-manager and tool caches that can be rebuilt. Coming soon." />;
}
