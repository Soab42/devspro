/**
 * The app's entire icon vocabulary, in one place.
 *
 * Nothing outside this file may import from `lucide-react`. Two reasons:
 *   1. Swapping an icon app-wide is a one-line change here.
 *   2. It keeps the "no emoji, SVG only" rule enforceable by grep.
 *
 * Named re-exports only — never `import * as Icons`, which defeats
 * tree-shaking and would pull all ~1770 icons into the bundle.
 *
 * NOTE ON NAMES: lucide-react v1 dropped the old numeric-suffix aliases.
 * If you port code written against v0.5xx, translate:
 *   Loader2 → LoaderCircle      CheckCircle2 → CircleCheck
 *   XCircle → CircleX           AlertTriangle → TriangleAlert
 *   MoreHorizontal → Ellipsis   Filter → Funnel
 *   FileCode2 → FileCode        Wand2 → WandSparkles
 */
export type { LucideIcon } from "lucide-react";

export {
  /* ---- module / navigation ---- */
  LayoutDashboard, // Dashboard
  Globe, // Web Server & SSL
  Container, // Docker
  Layers, // a compose stack (group of containers)
  FileUp, // load a compose file
  ArrowUpCircle, // compose up — apply the file's current definition
  Database, // Databases
  ScrollText, // Logs
  Mail, // Mail Catcher
  Package, // Apps
  Network, // Processes
  Workflow, // Workers
  CalendarClock, // Scheduled tasks
  DatabaseZap, // Redis module
  Boxes, // Redis: the Keys tab
  Send, // HTTP client
  Settings, // Settings
  Blocks, // Settings: Modules section

  /* ---- ai agents ---- */
  Bot, // AI agents module
  Sparkles, // launch an agent

  /* ---- ssh ---- */
  SquareTerminal, // SSH module
  FolderSync, // SFTP module
  Folder, // SFTP directory entry
  File, // SFTP file entry
  ArrowUp, // SFTP: go to the parent directory
  Unplug, // disconnect an SFTP session
  SquareSplitHorizontal, // terminal: split the pane side by side
  SquareSplitVertical, // terminal: split the pane top and bottom
  PanelLeft, // terminal: show / hide the follow-along file browser
  LocateFixed, // terminal browser: follow the shell's working directory
  FolderOpen, // terminal browser: cd the shell to this folder
  FolderInput, // terminal browser: open a folder by choosing or typing its path

  /* ---- remote desktop ---- */
  ScreenShare, // Remote Desktop module
  MonitorUp, // an RDP connection (opens in the system client)
  Keyboard, // send Ctrl+Alt+Del
  ClipboardPaste, // paste the local clipboard into the remote session
  Maximize, // viewer fullscreen
  Minimize, // leave viewer fullscreen
  Expand, // show the remote screen at actual size
  Shrink, // scale the remote screen to fit
  Plug, // connect

  /* ---- file manager ---- */
  Files, // File Manager module
  House, // place: home
  Image, // place: pictures
  Music, // place: music
  Film, // place: videos
  FilePlus, // new empty file
  LayoutGrid, // icon view
  List, // list view
  SquareArrowOutUpRight, // open with the default app

  /* ---- browser ---- */
  AppWindow, // Browser module
  Compass, // Safari-style skin
  Flame, // Firefox-style skin
  ZoomIn,
  ZoomOut,

  /* ---- env diff ---- */
  FileDiff, // Env Diff module
  BookUser, // Hosts file module
  ArrowRight, // copy a variable to the right-hand file
  ArrowLeft, // copy a variable to the left-hand file

  /* ---- dashboard metrics ---- */
  Cpu,
  Server, // System
  MemoryStick, // Memory
  Gpu, // GPU
  Thermometer, // Temperature
  HardDrive, // Storage / disk
  Activity, // Network throughput
  Clock, // Uptime
  ShieldCheck, // Health: nominal
  TriangleAlert, // Health: degraded

  /* ---- nginx / ssl ---- */
  ShieldAlert, // certificate expiring or invalid
  FileCode, // PHP version
  Hexagon, // Node version
  FolderPlus, // new project
  Wrench, // fix permissions
  Power, // enable / disable site
  SlidersHorizontal, // per-site limits (upload size, memory, timeouts)

  /* ---- chrome & controls ---- */
  X, // close
  ChevronUp, // sort ascending
  ChevronDown, // sort descending, and the Select caret
  ChevronsUpDown, // sortable but unsorted, and "expand all"
  ChevronsDownUp, // "fold all" over a list of collapsible sections
  ChevronRight, // disclosure
  ChevronLeft, // previous page (database browser)
  Table2, // a database table (database browser)
  Ellipsis, // overflow menu
  Menu, // sidebar drawer toggle
  PanelLeftClose,
  PanelLeftOpen,
  PanelRight, // top bar: show / hide the docked terminal
  PanelRightClose, // docked terminal: hide it
  Sun, // light theme
  Monitor, // system theme
  Moon, // dark theme
  Search,
  SearchX, // no search matches
  Funnel, // filter

  /* ---- actions ---- */
  RefreshCw, // refresh
  Plus, // add / create
  Trash2, // delete
  Pencil, // edit
  Play, // start
  Square, // stop
  RotateCw, // restart
  OctagonX, // force kill
  FileText, // view logs (row action; ScrollText is reserved for the module)
  ExternalLink,
  Copy,
  Check, // copied
  Download,
  Upload,
  Maximize2, // expand into modal
  WandSparkles, // suggest a value
  Terminal, // command
  KeyRound, // credentials
  Users, // manage users
  Eye,
  EyeOff,
  LogOut,
  Lock,

  /* ---- status & feedback ---- */
  LoaderCircle, // spinner / pending
  CircleCheck, // ok
  CircleX, // bad
  CircleAlert, // warning
  Circle, // neutral
  Info,
  Inbox, // default empty state
  PackageX, // dependency not installed
  PlugZap, // daemon unreachable

  /* ---- auth ---- */
  UserPlus,

  /* ---- browser ---- */
  Star, // bookmark this page (filled when bookmarked)
  Bookmark, // a saved bookmark
  Columns2, // split view; SFTP panes side by side
  Columns3, // file manager column view
  FileImage, // file manager: image kinds
  FileVideo, // file manager: video kinds
  FileAudio, // file manager: audio kinds
  FileArchive, // file manager: archives
  FileTerminal, // file manager: shell scripts
  FileLock, // file manager: lock files
  FileCog, // file manager: config files
  FolderSymlink, // file manager: a link to a folder
  StarOff, // file manager: remove from favourites
  Wifi, // host status strip: network
  Scissors, // cut to the file clipboard
  Rows2, // SFTP panes stacked
  ChevronsRightLeft, // fold an SFTP pane (side by side)
  ChevronsLeftRight, // unfold an SFTP pane (side by side)
  Code, // inspect / web inspector
  Printer,
  Smartphone, // device preset
  Clock3, // history
  PictureInPicture2, // pop a tab out into its own window

  /* ---- database browser ---- */
  Link2, // a foreign key relationship
  Rows3, // add a row to the grid
  Undo2, // revert a staged edit
  Save, // commit staged edits
  FileJson, // export as JSON
  FileSpreadsheet, // export as CSV
  LockOpen, // read & write mode (Lock, for read-only, is already exported above)

  /* ---- code editor ---- */
  ListOrdered, // go to line
  Redo2, // redo (Undo2 is exported above)

  /* ---- memory ---- */
  Eraser, // drop the disk cache

  /* ---- file manager: devices ---- */
  Usb, // USB drive or memory card
  Camera, // camera over gPhoto2
  Disc, // optical disc
  Cloud, // Google Drive and other online accounts
  Eject, // unmount, eject, safely remove

  /* ---- file manager: editor and git ---- */
  FoldVertical, // fold a block
  UnfoldVertical, // unfold a block
  MessageSquareCode, // toggle comment
  GitBranch, // branch in the status line
  GitCompare, // show changes inline
  GitCommitHorizontal, // one commit
  History, // file history
  ArrowDown, // next change
} from "lucide-react";

/* ---- composed icons ------------------------------------------------------
   Lucide has no PDF glyph, and its FileType "T" reads as a plain text file.
   This is the file outline with "PDF" lettered in strokes, built through
   Lucide's own factory so it sizes and themes like every other icon. */
import { createLucideIcon } from "lucide-react";

export const FilePdf = createLucideIcon("file-pdf", [
  ["path", { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z", key: "outline" }],
  ["path", { d: "M14 2v4a2 2 0 0 0 2 2h4", key: "fold" }],
  ["path", { d: "M7 18.5v-6h2a1.5 1.5 0 0 1 0 3H7", key: "p" }],
  ["path", { d: "M11.5 18.5v-6h1a3 3 0 0 1 0 6Z", key: "d" }],
  ["path", { d: "M16.5 18.5v-6h3", key: "f-stem" }],
  ["path", { d: "M16.5 15.25h2.25", key: "f-bar" }],
]);
