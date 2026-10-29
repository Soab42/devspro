import { useEffect, useState } from "react";
import { Icon, Select, Spinner, Stack, TextInput } from "../../../ui";
import { CircleAlert, CircleCheck, CircleX } from "../../../ui/icons";
import { liveVersions } from "../api";

/** How many of the newest versions the menu lists before "Other version…". */
const SHOWN = 5;

const LATEST = "latest";
const MANUAL = "manual";
const LOADING = "__loading";
const FAILED = "__failed";

/** The pick as the install needs it: `null` is Latest, and `valid` is false
    only for a typed version that isn't among the published ones. */
export interface LiveVersionChoice {
  version: string | null;
  valid: boolean;
}

type ListState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; versions: string[] }
  | { kind: "failed"; message: string };

/**
 * Version picker for an app with too many releases to list up front — Docker
 * Desktop has well over a hundred. The menu offers Latest (the package
 * manager's default, nothing checked), the five newest, and "Other version…",
 * which opens a box for any other one, checked against the full list as it's
 * typed.
 *
 * Nothing is fetched until the menu first opens: the list comes from the
 * package manager over the network, which the Apps page shouldn't wait on.
 */
export default function LiveVersionPicker({
  appId,
  appName,
  disabled,
  onChange,
}: {
  appId: string;
  appName: string;
  disabled?: boolean;
  onChange: (choice: LiveVersionChoice) => void;
}) {
  const [list, setList] = useState<ListState>({ kind: "idle" });
  const [selected, setSelected] = useState(LATEST);
  const [typed, setTyped] = useState("");

  function load() {
    if (list.kind === "loading" || list.kind === "ready") return;
    setList({ kind: "loading" });
    liveVersions(appId).then(
      (versions) => setList({ kind: "ready", versions }),
      (e) => setList({ kind: "failed", message: e instanceof Error ? e.message : String(e) }),
    );
  }

  const wanted = typed.trim();
  const found = list.kind === "ready" && list.versions.includes(wanted);

  useEffect(() => {
    if (selected === MANUAL) onChange({ version: wanted || null, valid: found });
    else onChange({ version: selected === LATEST ? null : selected, valid: true });
    // Report only when the pick changes; a new onChange identity is not one.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, wanted, found]);

  const options = [
    { value: LATEST, label: "Latest" },
    ...(list.kind === "ready"
      ? list.versions.slice(0, SHOWN).map((v) => ({ value: v, label: v }))
      : list.kind === "failed"
        ? [{ value: FAILED, label: "Couldn't load versions" }]
        : [{ value: LOADING, label: "Loading versions…" }]),
    { value: MANUAL, label: "Other version…" },
  ];

  function pick(value: string) {
    // The placeholder rows explain the list; they aren't choices.
    if (value === LOADING || value === FAILED) return;
    setSelected(value);
    if (value === MANUAL) load();
  }

  return (
    // Stacked — the menu, then the typed version beside its tick or cross — so
    // the Version column keeps the width the other rows already gave it.
    <Stack gap={2}>
      <Select
        size="sm"
        value={selected}
        onChange={pick}
        options={options}
        disabled={disabled}
        onOpenChange={(open) => open && load()}
        aria-label={`Version for ${appName}`}
      />
      {selected === MANUAL && (
        <Stack direction="row" gap={2} align="center">
          <TextInput
            inputSize="sm"
            mono
            className="u-flex-1"
            placeholder="e.g. 4.92.0"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            invalid={Boolean(wanted) && list.kind === "ready" && !found}
            aria-label={`${appName} version to install`}
            autoFocus
          />
          <ManualStatus appName={appName} wanted={wanted} list={list} found={found} />
        </Stack>
      )}
    </Stack>
  );
}

/** The tick or cross beside a typed version. Hovering it says which, in words. */
function ManualStatus({
  appName,
  wanted,
  list,
  found,
}: {
  appName: string;
  wanted: string;
  list: ListState;
  found: boolean;
}) {
  if (!wanted) return null;
  if (list.kind === "loading" || list.kind === "idle") {
    return <Spinner size="sm" label="Checking the version list" />;
  }
  // Colour from the table's own success/failure note classes, so the tick and
  // cross match the green and red an install result is written in.
  const [icon, colour, text] =
    list.kind === "failed"
      ? [CircleAlert, "u-muted", `Couldn't check ${wanted}: the version list didn't load (${list.message})`]
      : found
        ? [CircleCheck, "dt-cell-note-ok", `${appName} ${wanted} is available`]
        : [CircleX, "dt-cell-note-bad", `${appName} ${wanted} isn't available`];
  return (
    <span className={colour} title={text} role="img" aria-label={text}>
      <Icon icon={icon} size="sm" />
    </span>
  );
}
