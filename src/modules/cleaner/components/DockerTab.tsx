import { EmptyState } from "../../../ui";
import { Eraser } from "../../../ui/icons";

/** Placeholder; this tab is built in its own task. */
export default function DockerTab() {
  return <EmptyState icon={Eraser} size="md" title="Docker" description="Unused images, containers, volumes and build cache. Coming soon." />;
}
