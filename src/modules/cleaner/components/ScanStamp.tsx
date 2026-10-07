import { STALE_AFTER_MS } from "../tabState";
import { timeAgo } from "./overview/lastScan";

/** When the shown results were scanned, with a note when they are old. */
export default function ScanStamp({ at }: { at: number | null }) {
  if (at === null) return null;
  const old = Date.now() - at > STALE_AFTER_MS;
  return (
    <span className="u-muted">
      Scanned {timeAgo(new Date(at).toISOString())}.
      {old && " These results are more than a day old and may be out of date."}
    </span>
  );
}
