import { useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { FileText, MessageSquareText, Search } from "lucide-react";
import { fileAnchorId } from "./fileLinks";
import { buildQuickAccessResults } from "./quickAccessSearch";
import type { CommentThread, ReviewFile } from "./reviewData";

type QuickAccessProps = {
  open: boolean;
  allFiles: boolean;
  files: ReviewFile[];
  threads: CommentThread[];
  onClose: () => void;
};

export function QuickAccess({ allFiles, open, files, threads, onClose }: QuickAccessProps) {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const results = useMemo(
    () => buildQuickAccessResults({ query, files, threads }),
    [files, query, threads],
  );

  if (!open) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 bg-background/70 backdrop-blur-sm">
      <div className="fixed left-1/2 top-[12vh] grid max-h-[min(720px,76vh)] w-[min(760px,calc(100vw-2rem))] -translate-x-1/2 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-lg border bg-background shadow-lg">
        <div className="sticky top-0 z-10 border-b bg-background p-3">
          <div className="flex items-center gap-2 rounded-md border bg-muted/40 px-3">
            <Search className="size-4 text-muted-foreground" />
            <input
              autoFocus
              className="h-10 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search files and comments"
              value={query}
            />
          </div>
        </div>
        <div className="min-h-0 overflow-auto p-2">
          {results.length === 0 ? (
            <div className="p-6 text-center text-sm text-muted-foreground">
              No matching files or comments
            </div>
          ) : (
            results.map((result) => (
              <button
                className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm hover:bg-accent hover:text-accent-foreground"
                key={result.kind === "file" ? result.path : result.commentId}
                onClick={() => {
                  if (result.kind === "comment") {
                    if (result.isChanged) {
                      void navigate({
                        to: "/",
                        search: (previous) => ({
                          ...previous,
                          allFiles,
                          comment: result.threadId,
                        }),
                        hash: `comment-${result.threadId}`,
                      });
                    } else {
                      void navigate({
                        to: "/file",
                        search: (previous) => ({
                          ...previous,
                          allFiles,
                          comment: result.threadId,
                          path: result.path,
                        }),
                        hash: `comment-${result.threadId}`,
                      });
                    }
                  } else if (result.isChanged) {
                    void navigate({
                      to: "/",
                      search: (previous) => ({ ...previous, allFiles }),
                      hash: fileAnchorId(result.path),
                    });
                  } else {
                    void navigate({
                      to: "/file",
                      search: (previous) => ({ ...previous, allFiles, path: result.path }),
                    });
                  }
                  onClose();
                }}
                type="button"
              >
                {result.kind === "file" ? (
                  <FileText className="size-4 text-muted-foreground" />
                ) : (
                  <MessageSquareText className="size-4 text-muted-foreground" />
                )}
                <span className="min-w-0 flex-1 truncate font-mono">
                  {result.kind === "file" ? result.path : result.excerpt}
                </span>
                <span className="shrink-0 font-mono text-xs text-muted-foreground">
                  {result.kind === "file" ? result.status : result.lineLabel}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
