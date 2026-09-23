/**
 * DevsPro UI primitives.
 *
 * Rules for this directory:
 *   - Every component owns a sibling .css file, imported by the component
 *     itself. Deleting a primitive deletes its styles with it.
 *   - No primitive stylesheet may style a bare element selector or another
 *     primitive's class. Global element defaults live in src/styles/base.css.
 *   - Nothing here or in src/modules imports from "lucide-react" directly;
 *     the icon vocabulary is re-exported from ./icons.
 *
 * DELIBERATELY ABSENT — a Tooltip primitive. The only real need is showing the
 * full text of a value that has been truncated, and native `title=` already
 * covers that; IconButton sets it automatically from its required `label`. A
 * real tooltip needs collision detection and hover-intent timing, which is a
 * lot of surface area for that. Revisit only if a case needs rich, non-string
 * content — please don't reopen it just for consistency.
 */

export { Icon } from "./Icon";
export type { IconProps, IconSize } from "./Icon";

export { Button, IconButton } from "./Button";
export type {
  ButtonProps,
  ButtonSize,
  ButtonVariant,
  IconButtonProps,
} from "./Button";

export { Spinner, Skeleton } from "./Feedback";
export type { SpinnerProps, SkeletonProps } from "./Feedback";

export { StatusPill, Badge, Callout } from "./Status";
export type {
  StatusPillProps,
  StatusVariant,
  BadgeProps,
  BadgeTone,
  CalloutProps,
  CalloutTone,
} from "./Status";

/* CodeEditor is deliberately NOT re-exported here. It pulls in CodeMirror
   (~350 kB), and because every module imports its primitives from this barrel,
   one export line puts that in the initial bundle for every page — which
   silently undoes the lazy() boundary at its only call site. Import it directly:
   `import { CodeEditor } from "../../../ui/CodeEditor"`. */

export { Mono, CodeBlock, CopyButton } from "./Mono";
export type { MonoProps, CodeBlockProps, CopyButtonProps } from "./Mono";

export { Stack, EmptyState, Toolbar } from "./Layout";
export type { StackProps, EmptyStateProps, ToolbarProps } from "./Layout";

export { SplitView, SideList } from "./SplitView";
export type { SplitViewProps, SideListProps, SideListItem } from "./SplitView";

export { Card } from "./Card";
export type { CardProps, CardPadding } from "./Card";

export {
  Field,
  TextInput,
  TextArea,
  Switch,
  Checkbox,
  SearchInput,
  Form,
  FormRow,
  FormGrid,
} from "./Field";
export type {
  FieldProps,
  TextInputProps,
  TextAreaProps,
  SwitchProps,
  CheckboxProps,
  SearchInputProps,
  FormProps,
  FormRowProps,
  FormGridProps,
} from "./Field";

export { SegmentedControl } from "./SegmentedControl";
export type { SegmentedControlProps, SegmentOption } from "./SegmentedControl";

export { Select } from "./Select";
export type { SelectProps, SelectOption } from "./Select";

export { Tabs } from "./Tabs";
export type { TabsProps, TabDefinition } from "./Tabs";

export { DataTable } from "./DataTable";
export type {
  Column,
  ColumnAlign,
  ColumnPriority,
  DataTableProps,
  SortDirection,
  SortState,
} from "./DataTable";
export { sortRows, nextSort } from "./dataTableUtils";

export { Modal, ModalFooter } from "./Modal";
export type { ModalProps, ModalFooterProps, ModalSize } from "./Modal";

export { Menu, ActionGroup } from "./Menu";
export type { MenuProps, MenuItem } from "./Menu";

export { Meter, meterSeverity } from "./Meter";
export type { MeterProps, MeterSeverity } from "./Meter";

export { Sparkline } from "./Sparkline";
export type { SparklineProps } from "./Sparkline";

export { Page, statusOf } from "./page/Page";
export type { PageProps, PageStatus } from "./page/Page";
export { PageChromeContext, usePageChrome } from "./page/PageChrome";
export type { PageChrome } from "./page/PageChrome";

export { Section } from "./page/Section";
export type { SectionProps, SectionEmpty } from "./page/Section";

export { useResource } from "./page/useResource";
export type { Resource, ResourceOptions } from "./page/useResource";
