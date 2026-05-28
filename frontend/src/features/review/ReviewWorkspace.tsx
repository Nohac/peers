import { FileSidebar } from "./FileSidebar";
import { useReviewFiles, useThreadsForFile } from "./reviewData";
import { DiffViewer } from "../diff/DiffViewer";
import type { ReviewFile } from "./reviewData";

type ReviewWorkspaceProps = {
  allFiles: boolean;
  activeCommentId?: string;
};

export function ReviewWorkspace({ activeCommentId, allFiles }: ReviewWorkspaceProps) {
  const visibleFiles = useReviewFiles({ includeUnchangedFiles: allFiles });
  const changedFiles = visibleFiles.filter((file) => file.isChanged);

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)] border-t">
      <FileSidebar allFiles={allFiles} files={visibleFiles} />
      <section className="min-h-0 scroll-smooth overflow-auto bg-muted/20">
        <div className="space-y-4 p-4">
          {changedFiles.map((file) => (
            <ReviewDiffViewer
              activeCommentId={activeCommentId}
              allFiles={allFiles}
              file={file}
              key={file.path}
            />
          ))}
        </div>
      </section>
    </div>
  );
}

type ReviewDiffViewerProps = {
  allFiles: boolean;
  file: ReviewFile;
  activeCommentId?: string;
};

function ReviewDiffViewer({ activeCommentId, allFiles, file }: ReviewDiffViewerProps) {
  const threads = useThreadsForFile(file.path);

  return (
    <DiffViewer
      activeCommentId={activeCommentId}
      allFiles={allFiles}
      file={file}
      threads={threads}
    />
  );
}
