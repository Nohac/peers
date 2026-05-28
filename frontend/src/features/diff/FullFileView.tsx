import { MessageSquarePlus } from "lucide-react";
import { addedFileLines, type CommentThread, type ReviewFile } from "../review/reviewData";

type FullFileViewProps = {
  file: ReviewFile;
  threads: CommentThread[];
  activeCommentId?: string;
};

export function FullFileView({ file, threads, activeCommentId }: FullFileViewProps) {
  return (
    <div className="overflow-hidden rounded-md border bg-background font-mono text-xs">
      {addedFileLines.map((line, index) => (
        <button
          className="group flex min-h-8 w-full items-stretch bg-background text-left hover:bg-accent/60"
          key={`${file.path}-${index}`}
          type="button"
        >
          <span className="flex w-12 shrink-0 items-center justify-end border-r px-2 text-muted-foreground">
            {index + 1}
          </span>
          <span className="relative flex min-w-0 flex-1 items-center px-3">
            <MessageSquarePlus className="absolute left-1 size-3.5 opacity-0 group-hover:opacity-100" />
            <span className="truncate pl-4">{line}</span>
          </span>
        </button>
      ))}
      {threads.map((thread) => (
        <div
          className={[
            "border-t p-3 font-sans text-sm",
            activeCommentId === thread.id ? "bg-primary text-primary-foreground" : "bg-accent/50",
          ].join(" ")}
          key={thread.id}
        >
          <span className="font-mono font-medium">{thread.lineLabel}</span>
          <span
            className={[
              "ml-2",
              activeCommentId === thread.id ? "text-primary-foreground" : "text-muted-foreground",
            ].join(" ")}
          >
            {thread.comments[0]?.body}
          </span>
        </div>
      ))}
    </div>
  );
}
