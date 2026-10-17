import { ReactNode, useEffect, useRef, useState } from "react";
import { Button, Callout, EmptyState, Spinner } from "../../../ui";
import { FileAudio, SquareArrowOutUpRight } from "../../../ui/icons";
import { fmMediaUrl } from "../api";
import { entryIcon } from "../entryIcon";
import type { OpenFile } from "../openFiles";
import { errorMessage, viewerKind } from "../utils";
import FileTabBar from "./FileTabBar";
import ImageViewer from "./ImageViewer";
import TextViewer from "./TextViewer";

/** The `fmfile://` address the webview loads a picture, film or PDF from. */
function useMediaUrl(path: string, wanted: boolean) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!wanted) return;
    let cancelled = false;
    fmMediaUrl(path)
      .then((u) => !cancelled && setUrl(u))
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [path, wanted]);
  return { url, error };
}

/**
 * One open file, in whatever form suits it: the editor for text and source,
 * a picture, a player for audio and video, WebKit's PDF viewer. Every tab
 * stays mounted while another is in front, so an edit, a zoom level or a
 * paused film is where it was on the way back. A player is paused when its
 * tab goes to the back.
 */
export default function FileTab({
  file,
  active,
  shownPath,
  onDirty,
  onSaved,
  onReveal,
  onOpenExternal,
}: {
  file: OpenFile;
  active: boolean;
  shownPath: string;
  onDirty: (dirty: boolean) => void;
  onSaved: (message: string) => void;
  onReveal: () => void;
  onOpenExternal: () => void;
}) {
  const kind = viewerKind({ name: file.name, is_dir: false, is_symlink: false }) ?? "other";
  const media = useMediaUrl(file.path, kind !== "text" && kind !== "other");
  const player = useRef<HTMLMediaElement | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!active) player.current?.pause();
  }, [active]);

  const bar = (extra?: ReactNode) => (
    <FileTabBar path={file.path} shownPath={shownPath} onReveal={onReveal} onOpenExternal={onOpenExternal}>
      {extra}
    </FileTabBar>
  );
  const openExternal = (
    <Button variant="secondary" size="sm" icon={SquareArrowOutUpRight} onClick={onOpenExternal}>
      Open with default app
    </Button>
  );

  let body: ReactNode;
  if (kind === "text") {
    body = (
      <TextViewer path={file.path} name={file.name} shownPath={shownPath} onDirty={onDirty} onSaved={onSaved} onReveal={onReveal} onOpenExternal={onOpenExternal} />
    );
  } else if (kind === "other") {
    body = (
      <>
        {bar()}
        <div className="fm-viewer-empty">
          <EmptyState icon={entryIcon({ name: file.name, is_dir: false, is_symlink: false })} title={file.name} description="There is no preview for this kind of file." action={openExternal} size="md" />
        </div>
      </>
    );
  } else if (media.error || failed) {
    body = (
      <>
        {bar()}
        <Callout tone="warning" actions={openExternal}>
          {media.error ?? `This ${kind === "pdf" ? "PDF" : kind} could not be shown here — the format may need a codec the app's web view does not have.`}
        </Callout>
      </>
    );
  } else if (!media.url) {
    body = (
      <>
        {bar()}
        <div className="fm-viewer-empty">
          <Spinner size="md" />
        </div>
      </>
    );
  } else if (kind === "image") {
    body = <ImageViewer src={media.url} name={file.name} renderControls={bar} />;
  } else if (kind === "video") {
    body = (
      <>
        {bar()}
        <div className="fm-media-stage">
          <video ref={(el) => void (player.current = el)} src={media.url} controls className="fm-video" onError={() => setFailed(true)} />
        </div>
      </>
    );
  } else if (kind === "audio") {
    body = (
      <>
        {bar()}
        <div className="fm-media-stage fm-audio-stage">
          <EmptyState icon={FileAudio} title={file.name} size="md" />
          <audio ref={(el) => void (player.current = el)} src={media.url} controls className="fm-audio" onError={() => setFailed(true)} />
        </div>
      </>
    );
  } else {
    body = (
      <>
        {bar()}
        <iframe src={media.url} title={file.name} className="fm-pdf" />
      </>
    );
  }

  return <div className={active ? "fm-viewer-host" : "fm-viewer-host fm-hidden"}>{body}</div>;
}
