import { CheckCircle2, MessageSquareText } from "lucide-react";
import type { CommentThread } from "../review/reviewData";

type CommentPanelProps = {
  threads: CommentThread[];
  activeCommentId?: string;
};

export function CommentPanel({ threads, activeCommentId }: CommentPanelProps) {
  return (
    <aside className="min-h-0 border-l bg-background">
      <div className="border-b p-3">
        <div className="text-sm font-semibold">Comments</div>
        <div className="mt-1 text-xs text-muted-foreground">
          {threads.filter((thread) => !thread.resolved).length} unresolved
        </div>
      </div>
      <div className="min-h-0 space-y-3 overflow-auto p-3">
        {threads.map((thread) => (
          <section
            className={[
              "rounded-md border bg-card text-card-foreground",
              activeCommentId === thread.id ? "ring-2 ring-ring" : "",
            ].join(" ")}
            id={`comment-${thread.id}`}
            key={thread.id}
          >
            <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
              <div className="min-w-0 truncate font-mono text-xs font-medium">
                {thread.lineLabel}
              </div>
              {thread.resolved ? (
                <CheckCircle2 className="size-4 shrink-0 text-muted-foreground" />
              ) : (
                <MessageSquareText className="size-4 shrink-0 text-muted-foreground" />
              )}
            </div>
            <div className="space-y-3 p-3">
              {thread.comments.map((comment) => (
                <article className="text-sm" key={comment.id}>
                  <div className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
                    <span className="font-medium text-foreground">{comment.authorName}</span>
                    <span>{comment.authorKind}</span>
                  </div>
                  <p className="leading-6">{comment.body}</p>
                </article>
              ))}
              <div className="flex gap-2">
                <button className="h-8 rounded-md border px-3 text-xs hover:bg-accent">
                  Reply
                </button>
                <button className="h-8 rounded-md border px-3 text-xs hover:bg-accent">
                  {thread.resolved ? "Reopen" : "Resolve"}
                </button>
              </div>
            </div>
          </section>
        ))}
      </div>
    </aside>
  );
}
