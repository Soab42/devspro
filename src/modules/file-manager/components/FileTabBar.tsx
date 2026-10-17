import type { ReactNode } from "react";
import { CopyButton, IconButton } from "../../../ui";
import { FolderOpen, SquareArrowOutUpRight } from "../../../ui/icons";

/**
 * The strip along the top of every file tab: where the file is, the verbs
 * every kind of file shares, and whatever the viewer adds (Save, zoom).
 */
export default function FileTabBar({
  path,
  shownPath,
  notice,
  onReveal,
  onOpenExternal,
  children,
}: {
  path: string;
  /** The path as people read it: `~/…`, or from a device's name. */
  shownPath: string;
  notice?: string | null;
  onReveal: () => void;
  onOpenExternal: () => void;
  children?: ReactNode;
}) {
  return (
    <div className="fm-viewer-bar">
      <span className="fm-viewer-path" title={path}>
        {shownPath}
      </span>
      {notice && <span className="fm-viewer-notice u-secondary">{notice}</span>}
      {children}
      <IconButton icon={FolderOpen} label="Show in folder" size="sm" onClick={onReveal} />
      <IconButton icon={SquareArrowOutUpRight} label="Open with default app" size="sm" onClick={onOpenExternal} />
      <CopyButton value={path} label="Copy path" size="sm" />
    </div>
  );
}
