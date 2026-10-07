import { useState } from "react";
import { Link } from "react-router-dom";
import { Card } from "../../../ui";
import { Eraser } from "../../../ui/icons";
import { formatBytes } from "../../cleaner/cleaner";
import { readLastScan, timeAgo } from "../../cleaner/components/overview/lastScan";

/**
 * Reads the summary the Cleaner saved after its last scan. It never scans: a
 * scan walks the disk, which is not something to do every time the Dashboard opens.
 */
export function CleanerCard() {
  const [last] = useState(readLastScan);
  const total = last ? last.totals.safe + last.totals.review + last.totals.danger : 0;

  return (
    <Card title="Disk cleanup" icon={Eraser}>
      <Link to="/cleaner">
        {last ? `About ${formatBytes(total)} can be freed, scanned ${timeAgo(last.at)}` : "Scan for space you can free"}
      </Link>
    </Card>
  );
}
