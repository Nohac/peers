import { useLayoutEffect, useRef, useState } from "react";
import { MessageSquarePlus } from "lucide-react";
import { InlineCommentThread } from "../comments/InlineCommentThread";
import type { CommentThread } from "../review/reviewData";

export type FileLine = {
  lineNumber: number;
  text: string;
  tone?: "context" | "added" | "deleted";
};

type FileLineViewProps = {
  lines: FileLine[];
  threads: CommentThread[];
  activeCommentId?: string;
};

export function FileLineView({ lines, threads, activeCommentId }: FileLineViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [contentWidth, setContentWidth] = useState<number>();

  useLayoutEffect(() => {
    const measureContentWidth = () => {
      const scrollElement = scrollRef.current;
      if (!scrollElement) {
        return;
      }

      const nextWidth = Math.ceil(
        Math.max(scrollElement.clientWidth, fileLineContentWidth(scrollElement, lines)),
      );

      setContentWidth((currentWidth) => (currentWidth === nextWidth ? currentWidth : nextWidth));
    };

    measureContentWidth();

    const resizeObserver = new ResizeObserver(measureContentWidth);
    if (scrollRef.current) {
      resizeObserver.observe(scrollRef.current);
    }

    return () => resizeObserver.disconnect();
  }, [lines]);

  return (
    <div
      className="overflow-x-auto rounded-md border bg-background font-mono text-xs"
      ref={scrollRef}
    >
      <div className="min-w-full" style={contentWidth ? { width: `${contentWidth}px` } : undefined}>
        {lines.map((line) => {
          const lineThreads = threads.filter((thread) => thread.anchor.endLine === line.lineNumber);

          return (
            <div key={line.lineNumber}>
              <button
                className={[
                  "group flex min-h-8 w-full items-stretch text-left hover:bg-accent/60",
                  lineToneClass(line.tone ?? "context"),
                ].join(" ")}
                type="button"
              >
                <span className="flex w-12 shrink-0 items-center justify-end border-r px-2 text-muted-foreground">
                  {line.lineNumber}
                </span>
                <span className="relative flex min-w-0 flex-1 items-center px-3">
                  <MessageSquarePlus className="absolute left-1 size-3.5 opacity-0 group-hover:opacity-100" />
                  <span className="whitespace-pre pl-4">{line.text}</span>
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
    </div>
  );
}

const lineChromeWidth = 48 + 24 + 16 + 1;

function fileLineContentWidth(element: HTMLElement, lines: FileLine[]) {
  const context = document.createElement("canvas").getContext("2d");
  if (!context) {
    return element.clientWidth;
  }

  context.font = getComputedStyle(element).font;

  const textWidth = lines.reduce(
    (maxWidth, line) => Math.max(maxWidth, context.measureText(line.text).width),
    0,
  );

  return textWidth + lineChromeWidth;
}

function lineToneClass(tone: NonNullable<FileLine["tone"]>) {
  if (tone === "added") {
    return "bg-chart-2/10";
  }
  if (tone === "deleted") {
    return "bg-destructive/10";
  }
  return "bg-background";
}
