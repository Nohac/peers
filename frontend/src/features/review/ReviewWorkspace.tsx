import { FileCheck2, RefreshCw } from "lucide-react";
import { FileSidebar } from "./FileSidebar";
import { useReviewCommentActions, useReviewFiles, useThreadsForFile } from "./reviewData";
import { DiffViewer } from "../diff/DiffViewer";
import type { ReviewFile } from "./reviewData";

type ReviewWorkspaceProps = {
  allFiles: boolean;
  activeCommentId?: string;
};

export function ReviewWorkspace({ activeCommentId, allFiles }: ReviewWorkspaceProps) {
  const visibleFiles = useReviewFiles({ includeUnchangedFiles: allFiles });
  const changedFiles = visibleFiles.filter((file) => file.isChanged);
  const { refreshDiff } = useReviewCommentActions();

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)] border-t">
      <FileSidebar allFiles={allFiles} files={visibleFiles} />
      <section className="min-h-0 scroll-smooth overflow-auto bg-muted/20">
        {changedFiles.length > 0 ? (
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
        ) : (
          <EmptyDiffState onRefresh={refreshDiff} />
        )}
      </section>
    </div>
  );
}

type EmptyDiffStateProps = {
  onRefresh: () => void;
};

function EmptyDiffState({ onRefresh }: EmptyDiffStateProps) {
  return (
    <div className="flex min-h-full items-center justify-center p-6">
      <div className="w-full max-w-md rounded-md border bg-background p-6 text-center shadow-sm">
        <div className="mx-auto flex size-10 items-center justify-center rounded-md border bg-muted text-muted-foreground">
          <FileCheck2 className="size-5" />
        </div>
        <h2 className="mt-4 text-sm font-semibold">No file changes</h2>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          This review has no diffs to show. Refresh if you expected local edits to appear.
        </p>
        <button
          className="mt-5 inline-flex h-9 items-center gap-2 rounded-md border px-3 text-sm text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          onClick={onRefresh}
          type="button"
        >
          <RefreshCw className="size-3.5" />
          Refresh diff
        </button>
      </div>
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
