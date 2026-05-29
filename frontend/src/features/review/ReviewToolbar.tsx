import { GitCompareArrows, RefreshCw, Search } from "lucide-react";
import { useChangedFiles, useReviewCommentActions, useThreads } from "./reviewData";

type ReviewToolbarProps = {
  onQuickAccess: () => void;
};

export function ReviewToolbar({ onQuickAccess }: ReviewToolbarProps) {
  const changedFiles = useChangedFiles();
  const threads = useThreads();
  const { refreshDiff } = useReviewCommentActions();
  const unresolvedCount = threads.filter((thread) => !thread.resolved).length;
  const addedLines = changedFiles.reduce((total, file) => total + file.addedLines, 0);
  const removedLines = changedFiles.reduce((total, file) => total + file.removedLines, 0);
  const deltaLines = addedLines - removedLines;

  return (
    <header className="flex h-14 shrink-0 items-center justify-between gap-4 border-b bg-background px-4">
      <div className="flex min-w-0 items-center gap-3">
        <GitCompareArrows className="size-4 text-muted-foreground" />
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">main..current branch</div>
          <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
            <span className="truncate">
              {changedFiles.length} files changed, {unresolvedCount} unresolved comments
            </span>
            <span className="font-mono text-success">+{addedLines}</span>
            <span className="font-mono text-destructive">-{removedLines}</span>
            <span
              className={["font-mono", deltaLines >= 0 ? "text-success" : "text-destructive"].join(
                " ",
              )}
            >
              {formatSignedDelta(deltaLines)}
            </span>
          </div>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <button
          className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-sm text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          onClick={refreshDiff}
          type="button"
        >
          <RefreshCw className="size-3.5" />
          Refresh
        </button>
        <button
          className="inline-flex h-8 items-center gap-2 rounded-md border bg-background px-3 text-sm text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          onClick={onQuickAccess}
          type="button"
        >
          <Search className="size-3.5" />
          <span>Search</span>
          <kbd className="rounded border bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
            ⌘K
          </kbd>
        </button>
      </div>
    </header>
  );
}

function formatSignedDelta(delta: number) {
  return delta > 0 ? `+${delta}` : String(delta);
}
