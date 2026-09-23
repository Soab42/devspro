import { Fragment } from "react";
import { useModules } from "../../../app/ModulesContext";
import { MODULE_GROUPS, modules } from "../../../app/ModuleRegistry";
import { Section, Stack, Switch } from "../../../ui";
import { Blocks } from "../../../ui/icons";

/**
 * Switches every registered module on or off. A module that is off disappears
 * from the sidebar and the launcher, and — because the
 * registry's Component is a lazy() import — its code is never fetched, so the
 * app starts lighter the more of these are switched off.
 */
export default function ModulesSection() {
  const { disabled, isEnabled, setEnabled } = useModules();
  const enabledCount = modules.length - disabled.size;

  return (
    <Section
      title="Modules"
      icon={Blocks}
      description="Switch off what you don't use. A module that is off disappears from the sidebar and the launcher, and its code is never loaded — so the app starts lighter."
    >
      <Stack gap={4}>
        <p className="u-muted">
          {enabledCount} of {modules.length} modules on
        </p>
        <Stack gap={5}>
          {MODULE_GROUPS.map((group) => {
            const groupModules = modules.filter((m) => m.group === group.id);
            if (groupModules.length === 0) return null;
            return (
              <Fragment key={group.id}>
                {group.label && <p className="u-muted">{group.label}</p>}
                <Stack gap={4}>
                  {groupModules.map((m) => (
                    <Switch
                      key={m.id}
                      checked={isEnabled(m.id)}
                      onChange={(v) => setEnabled(m.id, v)}
                      label={m.label}
                      description={
                        m.core
                          ? `${m.description ?? ""} Always on.`.trim()
                          : m.description
                      }
                      disabled={m.core}
                    />
                  ))}
                </Stack>
              </Fragment>
            );
          })}
        </Stack>
      </Stack>
    </Section>
  );
}
