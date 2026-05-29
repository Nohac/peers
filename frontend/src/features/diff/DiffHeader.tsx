import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { MessageSquarePlus, PanelTopOpen } from "lucide-react";
import { CommentComposer } from "../comments/CommentComposer";
import { fullFileSearch } from "../review/fileLinks";
import { useReviewCommentActions, type CommentThread, type ReviewFile } from "../review/reviewData";

type DiffHeaderProps = {
  allFiles: boolean;
  file: ReviewFile;
  threads: CommentThread[];
};

export function DiffHeader({ allFiles, file, threads }: DiffHeaderProps) {
  const { createThread } = useReviewCommentActions();
  const [commenting, setCommenting] = useState(false);
  const firstThread = threads[0];

  return (
    <div className="sticky top-0 z-30 border-b bg-background shadow-sm">
      <div className="flex items-center justify-between gap-3 px-4 py-3">
        <div className="min-w-0">
          <div className="truncate font-mono text-sm font-semibold">{file.path}</div>
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <span>{file.status}</span>
            <span className="font-mono text-success">+{file.addedLines}</span>
            <span className="font-mono text-destructive">-{file.removedLines}</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs hover:bg-accent"
            onClick={() => setCommenting((open) => !open)}
            type="button"
          >
            <MessageSquarePlus className="size-3.5" />
            Comment on this file
          </button>
          {threads.length > 0 ? (
            <span className="rounded border px-2 py-1 text-xs text-muted-foreground">
              {threads.length} {threads.length === 1 ? "thread" : "threads"}
            </span>
          ) : null}
          <Link
            className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs no-underline hover:bg-accent"
            search={fullFileSearch({ allFiles, comment: firstThread?.id, path: file.path })}
            to="/file"
          >
            <PanelTopOpen className="size-3.5" />
            Full file
          </Link>
        </div>
      </div>
      {commenting ? (
        <div className="border-t bg-muted/20 p-3">
          <CommentComposer
            autoFocus
            onCancel={() => setCommenting(false)}
            onSubmit={(body) => {
              createThread({ body, path: file.path, scope: "file" });
              setCommenting(false);
            }}
            placeholder="Comment on this file"
          />
        </div>
      ) : null}
    </div>
  );
}
