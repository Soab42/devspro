import { GIT_CODE_LABEL, GitMark } from "../gitStatus";

/** The git letter at the end of a row, or a dot for a folder that holds changes. */
export default function GitBadge({ mark }: { mark: GitMark | null }) {
  if (!mark) return null;
  if (mark.inside) return <span className={`fm-git-dot fm-git-${mark.code}`} title={`Contains ${GIT_CODE_LABEL[mark.code].toLowerCase()} files`} />;
  return (
    <span className={`fm-git-badge fm-git-${mark.code}`} title={GIT_CODE_LABEL[mark.code]}>
      {mark.code}
    </span>
  );
}
