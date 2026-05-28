import { MessageSquarePlus } from "lucide-react";
import { modifiedDiff, type CommentThread } from "../review/reviewData";

type SideBySideDiffProps = {
  threads: CommentThread[];
  activeCommentId?: string;
};

export function SideBySideDiff({ threads, activeCommentId }: SideBySideDiffProps) {
  return (
    <div className="overflow-hidden rounded-md border bg-background font-mono text-xs">
      <div className="grid grid-cols-2 border-b bg-muted text-muted-foreground">
        <div className="border-r px-3 py-2">Old</div>
        <div className="px-3 py-2">New</div>
      </div>
      {modifiedDiff.map((line, index) => (
        <div
          className="group grid grid-cols-2"
          key={`${line.oldNumber}-${line.newNumber}-${index}`}
        >
          <DiffCell
            lineNumber={line.oldNumber}
            text={line.oldText}
            tone={line.kind === "deleted" ? "deleted" : "context"}
          />
          <DiffCell
            lineNumber={line.newNumber}
            text={line.newText}
            tone={line.kind === "added" ? "added" : "context"}
          />
        </div>
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

type DiffCellProps = {
  lineNumber?: number;
  text?: string;
  tone: "context" | "added" | "deleted";
};

function DiffCell({ lineNumber, text, tone }: DiffCellProps) {
  const toneClass =
    tone === "added" ? "bg-chart-2/10" : tone === "deleted" ? "bg-destructive/10" : "bg-background";

  return (
    <button className={`flex min-h-8 items-stretch border-r text-left ${toneClass}`} type="button">
      <span className="flex w-12 shrink-0 items-center justify-end border-r px-2 text-muted-foreground">
        {lineNumber ?? ""}
      </span>
      <span className="relative flex min-w-0 flex-1 items-center px-3">
        <MessageSquarePlus className="absolute left-1 size-3.5 opacity-0 group-hover:opacity-100" />
        <span className="truncate pl-4">{text ?? ""}</span>
      </span>
    </button>
  );
}
