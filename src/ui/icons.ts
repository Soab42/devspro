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
  Globe, // Nginx & SSL
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

  /* ---- ssh ---- */
  SquareTerminal, // SSH module
  Folder, // SFTP directory entry
  File, // SFTP file entry
  ArrowUp, // SFTP: go to the parent directory
  Unplug, // disconnect an SFTP session

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
  ChevronsUpDown, // sortable but unsorted
  ChevronRight, // disclosure
  ChevronLeft, // previous page (database browser)
  Table2, // a database table (database browser)
  Ellipsis, // overflow menu
  Menu, // sidebar drawer toggle
  PanelLeftClose,
  PanelLeftOpen,
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
} from "lucide-react";
