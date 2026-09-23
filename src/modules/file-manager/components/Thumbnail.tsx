import { useEffect, useState } from "react";
import { Icon, IconSize } from "../../../ui";
import { FileEntry, fmThumbnail } from "../api";
import { entryIcon, hasThumbnail } from "../entryIcon";

/* One in-flight or finished request per path and size, shared by every tile
   and card that shows the same file, and never more than a few renders at
   once so a folder of photos does not fork thirty thumbnailers. */
const cache = new Map<string, Promise<string | null>>();
const queue: Array<() => void> = [];
let running = 0;
const CONCURRENCY = 3;

function schedule<T>(job: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const start = () => {
      running += 1;
      job()
        .then(resolve, reject)
        .finally(() => {
          running -= 1;
          queue.shift()?.();
        });
    };
    if (running < CONCURRENCY) start();
    else queue.push(start);
  });
}

function thumbnailFor(entry: FileEntry, size: number): Promise<string | null> {
  const key = `${entry.path}\u0000${entry.modified}\u0000${size}`;
  let p = cache.get(key);
  if (!p) {
    p = schedule(() => fmThumbnail(entry.path, size).then((t) => t?.data_url ?? null)).catch(() => null);
    cache.set(key, p);
  }
  return p;
}

/**
 * A picture of the file when one can be made (images, and videos where a
 * thumbnailer is installed), otherwise the kind's icon.
 */
export default function Thumbnail({
  entry,
  size,
  iconSize = "lg",
  className,
}: {
  entry: FileEntry;
  /** Pixel width to render at; also the image's box. */
  size: number;
  iconSize?: IconSize;
  className?: string;
}) {
  const wants = hasThumbnail(entry);
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!wants) {
      setUrl(null);
      return;
    }
    let cancelled = false;
    thumbnailFor(entry, size).then((u) => {
      if (!cancelled) setUrl(u);
    });
    return () => {
      cancelled = true;
    };
  }, [entry, size, wants]);

  const icon = entryIcon(entry);
  const cls = ["fm-thumb", className].filter(Boolean).join(" ");
  if (url) return <img src={url} alt="" className={cls} draggable={false} />;
  return <Icon icon={icon} size={iconSize} className={[cls, "fm-thumb-icon", entry.is_dir ? "fm-col-icon-dir" : ""].join(" ").trim()} />;
}
