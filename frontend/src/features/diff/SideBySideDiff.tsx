import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { MessageSquarePlus } from "lucide-react";
import { InlineCommentThread } from "../comments/InlineCommentThread";
import { diffRowsForPath, type CommentThread, type DiffRow } from "../review/reviewData";

type SideBySideDiffProps = {
  path: string;
  threads: CommentThread[];
  activeCommentId?: string;
};

export function SideBySideDiff({ path, threads, activeCommentId }: SideBySideDiffProps) {
  const diffRows = diffRowsForPath(path);
  const oldPaneRef = useRef<HTMLDivElement>(null);
  const newPaneRef = useRef<HTMLDivElement>(null);
  const oldContentRef = useRef<HTMLDivElement>(null);
  const newContentRef = useRef<HTMLDivElement>(null);
  const syncingScrollRef = useRef(false);
  const [sharedContentWidth, setSharedContentWidth] = useState<number>();

  const syncPaneScroll = useCallback((source: "old" | "new") => {
    if (syncingScrollRef.current) {
      return;
    }

    const sourcePane = source === "old" ? oldPaneRef.current : newPaneRef.current;
    const targetPane = source === "old" ? newPaneRef.current : oldPaneRef.current;
    if (!sourcePane || !targetPane) {
      return;
    }

    syncingScrollRef.current = true;
    targetPane.scrollLeft = sourcePane.scrollLeft;
    requestAnimationFrame(() => {
      syncingScrollRef.current = false;
    });
  }, []);

  useLayoutEffect(() => {
    const measureContentWidth = () => {
      const oldPane = oldPaneRef.current;
      const newPane = newPaneRef.current;
      if (!oldPane || !newPane) {
        return;
      }

      const oldWidth = diffPaneContentWidth({
        pane: oldPane,
        rows: diffRows.map((row) => row.oldText ?? ""),
      });
      const newWidth = diffPaneContentWidth({
        pane: newPane,
        rows: diffRows.map((row) => row.newText ?? ""),
      });

      const nextWidth = Math.ceil(
        Math.max(oldPane.clientWidth, newPane.clientWidth, oldWidth, newWidth),
      );

      setSharedContentWidth((currentWidth) =>
        currentWidth === nextWidth ? currentWidth : nextWidth,
      );
    };

    measureContentWidth();

    const resizeObserver = new ResizeObserver(measureContentWidth);
    if (oldPaneRef.current) {
      resizeObserver.observe(oldPaneRef.current);
    }
    if (newPaneRef.current) {
      resizeObserver.observe(newPaneRef.current);
    }

    return () => resizeObserver.disconnect();
  }, [diffRows]);

  return (
    <div className="w-full overflow-hidden rounded-md border bg-background font-mono text-xs">
      <div className="grid w-full grid-cols-2 border-b bg-muted text-muted-foreground">
        <div className="min-w-0 border-r px-3 py-2">Old</div>
        <div className="min-w-0 px-3 py-2">New</div>
      </div>
      <div className="grid w-full grid-cols-2">
        <DiffPane
          activeCommentId={activeCommentId}
          onScroll={() => syncPaneScroll("old")}
          contentRef={oldContentRef}
          contentWidth={sharedContentWidth}
          paneRef={oldPaneRef}
          rows={diffRows}
          side="old"
          threads={threads}
        />
        <DiffPane
          activeCommentId={activeCommentId}
          onScroll={() => syncPaneScroll("new")}
          contentRef={newContentRef}
          contentWidth={sharedContentWidth}
          paneRef={newPaneRef}
          rows={diffRows}
          side="new"
          threads={threads}
        />
      </div>
    </div>
  );
}

type DiffPaneContentWidthInput = {
  pane: HTMLElement;
  rows: string[];
};

const lineChromeWidth = 48 + 24 + 16 + 1;

function diffPaneContentWidth({ pane, rows }: DiffPaneContentWidthInput) {
  const context = document.createElement("canvas").getContext("2d");
  if (!context) {
    return pane.clientWidth;
  }

  context.font = getComputedStyle(pane).font;

  const textWidth = rows.reduce(
    (maxWidth, row) => Math.max(maxWidth, context.measureText(row).width),
    0,
  );

  return textWidth + lineChromeWidth;
}

type DiffPaneProps = {
  activeCommentId?: string;
  contentRef: React.RefObject<HTMLDivElement | null>;
  contentWidth?: number;
  onScroll: () => void;
  paneRef: React.RefObject<HTMLDivElement | null>;
  rows: DiffRow[];
  side: "old" | "new";
  threads: CommentThread[];
};

function DiffPane({
  activeCommentId,
  contentRef,
  contentWidth,
  onScroll,
  paneRef,
  rows,
  side,
  threads,
}: DiffPaneProps) {
  return (
    <div
      className="min-w-0 overflow-x-auto border-r last:border-r-0"
      onScroll={onScroll}
      ref={paneRef}
    >
      <div
        className="w-max min-w-full"
        ref={contentRef}
        style={contentWidth ? { width: `${contentWidth}px` } : undefined}
      >
        {rows.map((row, index) => {
          const lineNumber = side === "old" ? row.oldNumber : row.newNumber;
          const text = side === "old" ? row.oldText : row.newText;
          const tone =
            side === "old" && row.tone === "deleted"
              ? "deleted"
              : side === "new" && row.tone === "added"
                ? "added"
                : "context";
          const lineThreads = threads.filter(
            (thread) => thread.anchor.side === side && thread.anchor.endLine === lineNumber,
          );

          return (
            <div key={`${side}-${row.oldNumber}-${row.newNumber}-${index}`}>
              <DiffCell lineNumber={lineNumber} text={text} tone={tone} />
              {lineThreads.length > 0 ? (
                <InlineThreadStack activeCommentId={activeCommentId} threads={lineThreads} />
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

type InlineThreadStackProps = {
  threads: CommentThread[];
  activeCommentId?: string;
};

function InlineThreadStack({ threads, activeCommentId }: InlineThreadStackProps) {
  return (
    <div className="space-y-3 border-r p-3 font-sans">
      {threads.map((thread) => (
        <InlineCommentThread
          active={activeCommentId === thread.id}
          key={thread.id}
          thread={thread}
        />
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
    <button
      className={`group flex min-h-8 w-full min-w-0 items-stretch border-r text-left hover:bg-accent/60 ${toneClass}`}
      type="button"
    >
      <span className="flex w-12 shrink-0 items-center justify-end border-r px-2 text-muted-foreground">
        {lineNumber ?? ""}
      </span>
      <span className="relative flex min-w-0 flex-1 items-center px-3">
        <MessageSquarePlus className="absolute left-1 size-3.5 opacity-0 group-hover:opacity-100" />
        <span className="whitespace-pre pl-4">{text ?? ""}</span>
      </span>
    </button>
  );
}
