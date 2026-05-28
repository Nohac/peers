import { CommentPanel } from "../comments/CommentPanel";
import { FullFileView } from "../diff/FullFileView";
import { useReviewFile, useThreadsForFile } from "./reviewData";

type FullFileRouteViewProps = {
  path: string;
  activeCommentId?: string;
};

export function FullFileRouteView({ path, activeCommentId }: FullFileRouteViewProps) {
  const file = useReviewFile(path);
  const fileThreads = useThreadsForFile(file.path);

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_340px] border-t">
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
      <CommentPanel activeCommentId={activeCommentId} threads={fileThreads} />
    </div>
  );
}
