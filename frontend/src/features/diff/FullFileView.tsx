import { MessageSquarePlus } from "lucide-react";
import { InlineCommentThread } from "../comments/InlineCommentThread";
import { addedFileLines, type CommentThread, type ReviewFile } from "../review/reviewData";

type FullFileViewProps = {
  file: ReviewFile;
  threads: CommentThread[];
  activeCommentId?: string;
};

export function FullFileView({ file, threads, activeCommentId }: FullFileViewProps) {
  return (
    <div className="overflow-hidden rounded-md border bg-background font-mono text-xs">
      {addedFileLines.map((line, index) => {
        const lineNumber = index + 1;
        const lineThreads = threads.filter((thread) => thread.anchor.endLine === lineNumber);

        return (
          <div key={`${file.path}-${index}`}>
            <button
              className="group flex min-h-8 w-full items-stretch bg-background text-left hover:bg-accent/60"
              type="button"
            >
              <span className="flex w-12 shrink-0 items-center justify-end border-r px-2 text-muted-foreground">
                {lineNumber}
              </span>
              <span className="relative flex min-w-0 flex-1 items-center px-3">
                <MessageSquarePlus className="absolute left-1 size-3.5 opacity-0 group-hover:opacity-100" />
                <span className="truncate pl-4">{line}</span>
              </span>
            </button>
            {lineThreads.length > 0 ? (
              <div className="space-y-3 border-t bg-muted/30 p-3 pl-16 font-sans">
                {lineThreads.map((thread) => (
                  <InlineCommentThread
                    active={activeCommentId === thread.id}
                    key={thread.id}
                    thread={thread}
                  />
                ))}
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}
