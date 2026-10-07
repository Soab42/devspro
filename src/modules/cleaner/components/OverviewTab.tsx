import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function OverviewTab() {
  return <EmptyState icon={Eraser} size="md" title="Overview" description="A summary of what can be cleaned, with the total space it would free. Coming soon." />;
}
