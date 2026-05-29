import { useState } from "react";
import { Outlet, useSearch } from "@tanstack/react-router";
import { useHotkeys } from "react-hotkeys-hook";
import { QuickAccess } from "./QuickAccess";
import { ReviewToolbar } from "./ReviewToolbar";
import { useReviewFiles, useThreads } from "./reviewData";

export function ReviewShell() {
  const [quickAccessOpen, setQuickAccessOpen] = useState(false);
  const search = useSearch({ from: "__root__" });
  const allFiles = search.allFiles;
  const files = useReviewFiles({ includeUnchangedFiles: allFiles });
  const threads = useThreads();

  useHotkeys(
    "mod+k",
    (event) => {
      event.preventDefault();
      setQuickAccessOpen((open) => !open);
    },
    {
      enableOnFormTags: true,
      preventDefault: true,
    },
  );

  useHotkeys("esc", () => setQuickAccessOpen(false), {
    enabled: quickAccessOpen,
    enableOnFormTags: true,
  });

  return (
    <main className="flex h-screen min-h-0 flex-col bg-background text-foreground">
      <ReviewToolbar onQuickAccess={() => setQuickAccessOpen(true)} />
      <Outlet />
      <QuickAccess
        allFiles={allFiles}
        files={files}
        onClose={() => setQuickAccessOpen(false)}
        open={quickAccessOpen}
        threads={threads}
      />
    </main>
  );
}
