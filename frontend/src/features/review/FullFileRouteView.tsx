import { FullFileView } from "../diff/FullFileView";
import { FileSidebar } from "./FileSidebar";
import { useReviewFile, useReviewFiles, useThreadsForFile } from "./reviewData";

type FullFileRouteViewProps = {
  path: string;
  allFiles: boolean;
  activeCommentId?: string;
};

export function FullFileRouteView({ path, activeCommentId, allFiles }: FullFileRouteViewProps) {
  const visibleFiles = useReviewFiles({ includeUnchangedFiles: allFiles });
  const file = useReviewFile(path);
  const fileThreads = useThreadsForFile(file.path);

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)] border-t">
      <FileSidebar allFiles={allFiles} files={visibleFiles} />
      <section className="min-h-0 overflow-auto bg-muted/20">
        <header className="border-b bg-background px-4 py-3">
          <div className="min-w-0">
            <div className="truncate font-mono text-sm font-semibold">{file.path}</div>
            <div className="text-xs text-muted-foreground">Full file</div>
          </div>
        </header>
        <div className="p-4">
          <FullFileView activeCommentId={activeCommentId} file={file} threads={fileThreads} />
        </div>
      </section>
    </div>
  );
}
