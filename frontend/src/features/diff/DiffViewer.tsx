import { type CommentThread, type ReviewFile } from "../review/reviewData";
import { fileAnchorId } from "../review/fileLinks";
import { DiffHeader } from "./DiffHeader";
import { GitDiffView } from "./GitDiffView";

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
      className="relative scroll-mt-4 rounded-md border bg-background"
      id={fileAnchorId(file.path)}
    >
      <DiffHeader allFiles={allFiles} file={file} threads={threads} />
      <div className="p-4">
        <GitDiffView
          activeCommentId={activeCommentId}
          file={file}
          mode={sideBySide ? "split" : "unified"}
          threads={threads}
        />
      </div>
    </section>
  );
}
