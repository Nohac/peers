import { diffLinesForPath, type CommentThread, type ReviewFile } from "../review/reviewData";
import { fileAnchorId } from "../review/fileLinks";
import { DiffHeader } from "./DiffHeader";
import { FullFileView } from "./FullFileView";
import { SideBySideDiff } from "./SideBySideDiff";

type DiffViewerProps = {
  allFiles: boolean;
  file: ReviewFile;
  threads: CommentThread[];
  activeCommentId?: string;
};

export function DiffViewer({ activeCommentId, allFiles, file, threads }: DiffViewerProps) {
  const sideBySide = file.status === "modified" || file.status === "renamed";

  return (
    <section
      className="scroll-mt-4 overflow-hidden rounded-md border bg-background"
      id={fileAnchorId(file.path)}
    >
      <DiffHeader allFiles={allFiles} file={file} threads={threads} />
      <div className="p-4">
        {sideBySide ? (
          <SideBySideDiff activeCommentId={activeCommentId} path={file.path} threads={threads} />
        ) : (
          <FullFileView
            activeCommentId={activeCommentId}
            file={file}
            lines={diffLinesForPath(file.path)}
            threads={threads}
          />
        )}
      </div>
    </section>
  );
}
