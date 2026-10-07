import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function ProjectsTab() {
  return <EmptyState icon={Eraser} size="md" title="Project junk" description="node_modules, build output and other artifacts in idle projects. Coming soon." />;
}
