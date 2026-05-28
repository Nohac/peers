import { Link } from "@tanstack/react-router";
import { MessageSquareText, PanelTopOpen } from "lucide-react";
import { fullFileSearch } from "../review/fileLinks";
import type { CommentThread, ReviewFile } from "../review/reviewData";

type DiffHeaderProps = {
  allFiles: boolean;
  file: ReviewFile;
  threads: CommentThread[];
};

export function DiffHeader({ allFiles, file, threads }: DiffHeaderProps) {
  const firstThread = threads[0];

  return (
    <div className="flex items-center justify-between gap-3 border-b bg-background px-4 py-3">
      <div className="min-w-0">
        <div className="truncate font-mono text-sm font-semibold">{file.path}</div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span>{file.status}</span>
          <span className="font-mono text-success">+{file.addedLines}</span>
          <span className="font-mono text-destructive">-{file.removedLines}</span>
        </div>
      </div>
      <div className="flex items-center gap-2">
        {firstThread ? (
          <Link
            className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs no-underline hover:bg-accent"
            hash={`comment-${firstThread.id}`}
            search={{ allFiles, comment: firstThread.id }}
            to="/"
          >
            <MessageSquareText className="size-3.5" />
            {threads.length}
          </Link>
        ) : (
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs opacity-50"
            disabled
            type="button"
          >
            <MessageSquareText className="size-3.5" />
            {threads.length}
          </button>
        )}
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
  );
}
