import type { LucideIcon } from "../../ui/icons";
import {
  File,
  FileArchive,
  FileAudio,
  FileCode,
  FileCog,
  FileImage,
  FileJson,
  FileLock,
  FileSpreadsheet,
  FileTerminal,
  FilePdf,
  FileText,
  FileVideo,
  Folder,
  FolderSymlink,
} from "../../ui/icons";
import type { FileEntry } from "./api";
import { extensionOf, fileKind } from "./utils";

const ICON_BY_KIND: Record<string, LucideIcon> = {
  Folder: Folder,
  "Folder (link)": FolderSymlink,
  TypeScript: FileCode,
  JavaScript: FileCode,
  Rust: FileCode,
  PHP: FileCode,
  Python: FileCode,
  Go: FileCode,
  Ruby: FileCode,
  HTML: FileCode,
  CSS: FileCode,
  SCSS: FileCode,
  SQL: FileCode,
  JSON: FileJson,
  YAML: FileCog,
  TOML: FileCog,
  Config: FileCog,
  "Env file": FileCog,
  Dockerfile: FileCog,
  Makefile: FileTerminal,
  "Shell script": FileTerminal,
  "Lock file": FileLock,
  Markdown: FileText,
  Text: FileText,
  Log: FileText,
  Document: FileText,
  PDF: FilePdf,
  Archive: FileArchive,
  Audio: FileAudio,
  Video: FileVideo,
  "PNG image": FileImage,
  "JPEG image": FileImage,
  "GIF image": FileImage,
  "WebP image": FileImage,
  "SVG image": FileImage,
};

const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif", "tiff", "tif"]);
const VIDEO_EXTS = new Set(["mp4", "mkv", "webm", "mov", "avi", "m4v", "ogv", "wmv"]);
const SHEET_EXTS = new Set(["csv", "tsv", "xlsx", "xls", "ods"]);

/** The icon that tells one kind of file from another at a glance. */
export function entryIcon(entry: Pick<FileEntry, "name" | "is_dir" | "is_symlink">): LucideIcon {
  const kind = fileKind(entry);
  if (kind === "Folder (link)") return FolderSymlink;
  if (entry.is_dir) return Folder;
  const ext = extensionOf(entry.name);
  if (SHEET_EXTS.has(ext)) return FileSpreadsheet;
  if (IMAGE_EXTS.has(ext)) return FileImage;
  if (VIDEO_EXTS.has(ext)) return FileVideo;
  return ICON_BY_KIND[kind.replace(" (link)", "")] ?? File;
}

/** Whether a thumbnail is worth asking the backend for. */
export function hasThumbnail(entry: Pick<FileEntry, "name" | "is_dir">): boolean {
  if (entry.is_dir) return false;
  const ext = extensionOf(entry.name);
  return IMAGE_EXTS.has(ext) || VIDEO_EXTS.has(ext);
}
