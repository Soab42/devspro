import { useState } from "react";
import { useModule } from "../../../app/ModuleRegistry";
import { Badge, Button, Callout, IconButton, Menu, Page, SearchInput, Section, SegmentedControl, Toolbar } from "../../../ui";
import { Copy, Plus } from "../../../ui/icons";
import { CommandList } from "../components/CommandList";
import { useCommandRunner } from "../components/useCommandRunner";
import { SCOPE_OPTIONS, copyText, useLibraryView } from "../components/useLibraryView";
import { useCommandLibrary } from "../useCommandLibrary";

/**
 * The whole command library: browse it, search it, and keep your own. The
 * same library sits beside the terminal, where a click types a command in.
 */
export default function CommandsPage() {
  const module = useModule("commands");
  const library = useCommandLibrary();
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const view = useLibraryView(library, setError);
  const runner = useCommandRunner({
    copy: (text) =>
      void copyText(text).then(
        () => setCopied(text),
        (e) => setError(String(e)),
      ),
  });

  const globalCount = library.commands.filter((c) => c.scope === "global").length;

  return (
    <Page
      title={module.label}
      icon={module.icon}
      description={module.description}
      status={library.loading ? { kind: "loading" } : undefined}
      meta={
        !library.loading && (
          <>
            <Badge tone="neutral">{globalCount} global</Badge>
            <Badge tone="accent">{library.personal.length} personal</Badge>
          </>
        )
      }
      actions={
        <Button variant="primary" size="sm" icon={Plus} onClick={view.addNew}>
          New command
        </Button>
      }
    >
      {(error ?? library.error) && (
        <Callout tone="danger" onDismiss={() => setError(null)}>
          {error ?? library.error}
        </Callout>
      )}
      {copied && (
        <Callout tone="success" onDismiss={() => setCopied(null)}>
          Copied <code>{copied}</code>
        </Callout>
      )}
      <Section
        toolbar={
          <Toolbar end={<SegmentedControl ariaLabel="Which commands" size="sm" value={view.scope} onChange={view.setScope} options={SCOPE_OPTIONS} />}>
            <SearchInput value={view.query} onChange={view.setQuery} placeholder="Search by name, command, category or tag…" autoFocus />
          </Toolbar>
        }
        isEmpty={!library.loading && view.matches === 0}
        empty={{
          title: view.scope === "personal" && !view.query ? "No personal commands yet" : "Nothing matches",
          description:
            view.scope === "personal" && !view.query
              ? "Add the commands you run often, or save a copy of a global one to change it."
              : "Try fewer or different words.",
        }}
      >
        <CommandList
          sections={view.sections}
          searching={view.query.trim() !== ""}
          storageKey="devspro.commands.page.open"
          pickLabel="Copy"
          onPick={(c) => runner.start(c, "copy")}
          actions={(c) => (
            <>
              <IconButton icon={Copy} label="Copy" size="sm" onClick={() => runner.start(c, "copy")} />
              <Menu items={view.menuItems(c)} label="More" size="sm" align="end" />
            </>
          )}
        />
      </Section>
      {runner.element}
      {view.formElement}
    </Page>
  );
}
