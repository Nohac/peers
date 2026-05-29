import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { generateDiffFile } from "@git-diff-view/file";
import {
  DiffModeEnum,
  DiffViewWithMultiSelect,
  SplitSide,
  type DiffFile,
  type DiffViewWithMultiSelectRef,
  type MultiSelectResult,
} from "@git-diff-view/react";
import { CommentComposer } from "../comments/CommentComposer";
import { InlineCommentThread } from "../comments/InlineCommentThread";
import {
  fileContentForPath,
  useReviewCommentActions,
  type CommentThread,
  type FileSide,
  type ReviewFile,
} from "../review/reviewData";

type GitDiffViewProps = {
  activeCommentId?: string;
  expandAllContext?: boolean;
  file: ReviewFile;
  mode?: "split" | "unified";
  threads: CommentThread[];
};

type ThreadExtendData = {
  activeCommentId?: string;
  threads: CommentThread[];
};

type SelectedRange = {
  endLine: number;
  side: FileSide;
  startLine: number;
};

type WidgetStore = {
  getReadonlyState: () => {
    setWidget: (input: { lineNumber?: number; side?: SplitSide }) => void;
  };
};

export function GitDiffView({
  activeCommentId,
  expandAllContext = false,
  file,
  mode = "split",
  threads,
}: GitDiffViewProps) {
  const { createThread } = useReviewCommentActions();
  const containerRef = useRef<HTMLDivElement>(null);
  const diffViewRef = useRef<DiffViewWithMultiSelectRef>(null);
  const widgetStoreRef = useRef<WidgetStore | undefined>(undefined);
  const [activeWidgetRange, setActiveWidgetRange] = useState<SelectedRange>();
  const content = fileContentForPath(file.path);
  const oldContent = file.status === "added" ? "" : joinFileLines(content?.old);
  const newContent = file.status === "deleted" ? "" : joinFileLines(content?.new ?? content?.old);
  const language = languageForPath(file.path);
  const diffFile = useMemo(
    () =>
      buildDiffFile({
        expandAllContext,
        language,
        mode,
        newContent,
        oldContent,
        path: file.path,
      }),
    [expandAllContext, file.path, language, mode, newContent, oldContent],
  );
  const extendData = useMemo(
    () => threadExtendData(threads, activeCommentId),
    [activeCommentId, threads],
  );
  const highlightedRanges = useMemo(() => commentRanges(threads), [threads]);

  useLayoutEffect(() => {
    const animationFrame = requestAnimationFrame(() => {
      highlightCommentRanges(containerRef.current, highlightedRanges);
    });

    return () => cancelAnimationFrame(animationFrame);
  }, [highlightedRanges]);

  return (
    <div
      className="peers-git-diff-view overflow-auto rounded-md border bg-background font-mono text-xs"
      ref={containerRef}
    >
      <DiffViewWithMultiSelect<ThreadExtendData>
        ref={diffViewRef}
        diffFile={diffFile}
        diffViewAddWidget
        diffViewFontSize={12}
        diffViewHighlight
        diffViewMode={mode === "split" ? DiffModeEnum.Split : DiffModeEnum.Unified}
        diffViewTheme="light"
        enableMultiSelect
        extendData={extendData}
        onCreateUseWidgetHook={(hook) => {
          widgetStoreRef.current = hook;
        }}
        onMultiSelectComplete={(result) => {
          const range = rangeFromSelection(result);
          setActiveWidgetRange(range);
          requestAnimationFrame(() => {
            widgetStoreRef.current?.getReadonlyState().setWidget({
              lineNumber: range.endLine,
              side: fileSideToSplitSide(range.side),
            });
          });
        }}
        onAddWidgetClick={({ fromLineNumber, lineNumber, side }) =>
          setActiveWidgetRange((currentRange) => {
            const nextRange = normalizeSelectedRange({
              endLine: lineNumber,
              side: splitSideToFileSide(side),
              startLine: fromLineNumber ?? lineNumber,
            });

            if (
              currentRange?.side === nextRange.side &&
              currentRange.endLine === nextRange.endLine &&
              currentRange.startLine !== currentRange.endLine &&
              nextRange.startLine === nextRange.endLine
            ) {
              return currentRange;
            }

            return nextRange;
          })
        }
        renderExtendLine={({ data }) =>
          data ? (
            <InlineThreadStack activeCommentId={data.activeCommentId} threads={data.threads} />
          ) : null
        }
        renderWidgetLine={({ fromLineNumber, lineNumber, onClose, side }) => {
          const fallbackRange = normalizeSelectedRange({
            endLine: lineNumber,
            side: splitSideToFileSide(side),
            startLine: fromLineNumber,
          });
          const range =
            activeWidgetRange?.side === fallbackRange.side &&
            activeWidgetRange.endLine === fallbackRange.endLine
              ? activeWidgetRange
              : fallbackRange;

          return (
            <div className="bg-muted/30 p-3 font-sans">
              <CommentComposer
                autoFocus
                onCancel={() => {
                  setActiveWidgetRange(undefined);
                  onClose();
                }}
                onSubmit={(body) => {
                  createThread({
                    body,
                    endLine: range.endLine,
                    path: file.path,
                    scope: "line",
                    side: range.side,
                    startLine: range.startLine,
                  });
                  diffViewRef.current?.clearSelection();
                  diffViewRef.current?.setPreselectedLines({ new: [], old: [] });
                  window.getSelection()?.removeAllRanges();
                  setActiveWidgetRange(undefined);
                  onClose();
                }}
              />
            </div>
          );
        }}
      />
    </div>
  );
}

function commentRanges(threads: CommentThread[]) {
  return threads.flatMap((thread) => {
    if (
      thread.scope !== "line" ||
      !isMultiLineRange({
        endLine: thread.anchor.endLine,
        side: thread.anchor.side,
        startLine: thread.anchor.startLine,
      })
    ) {
      return [];
    }

    return {
      endLine: thread.anchor.endLine,
      side: thread.anchor.side,
      startLine: thread.anchor.startLine,
    };
  });
}

function isMultiLineRange(range: SelectedRange) {
  return range.startLine !== range.endLine;
}

function normalizeSelectedRange(range: SelectedRange): SelectedRange {
  return {
    endLine: Math.max(range.startLine, range.endLine),
    side: range.side,
    startLine: Math.min(range.startLine, range.endLine),
  };
}

function rangeFromSelection(result: MultiSelectResult): SelectedRange {
  return normalizeSelectedRange({
    endLine: result.range.endLineNumber,
    side: result.range.side,
    startLine: result.range.startLineNumber,
  });
}

type BuildDiffFileInput = {
  expandAllContext: boolean;
  language: string;
  mode: "split" | "unified";
  newContent: string;
  oldContent: string;
  path: string;
};

function buildDiffFile({
  expandAllContext,
  language,
  mode,
  newContent,
  oldContent,
  path,
}: BuildDiffFileInput): DiffFile {
  const diffFile = generateDiffFile(
    path,
    oldContent,
    path,
    newContent,
    language,
    language,
    { context: 4 },
    path,
  );

  diffFile.initTheme("light");
  diffFile.init();
  diffFile.buildSplitDiffLines();
  diffFile.buildUnifiedDiffLines();

  if (expandAllContext) {
    diffFile.onAllExpand(mode);
  }

  return diffFile;
}

function joinFileLines(lines: string[] | undefined) {
  return lines?.join("\n") ?? "";
}

function highlightCommentRanges(container: HTMLElement | null, ranges: SelectedRange[]) {
  if (!container) {
    return;
  }

  for (const element of container.querySelectorAll<HTMLElement>("[data-peers-range-decoration]")) {
    element.classList.remove("peers-comment-range-box", "peers-comment-range-rail");
    element.removeAttribute("data-peers-range-decoration");
  }

  for (const range of ranges) {
    highlightSplitRange(container, range);
    highlightUnifiedRange(container, range);
    highlightRangeCommentBox(container, range);
  }
}

function highlightSplitRange(container: HTMLElement, range: SelectedRange) {
  for (const row of container.querySelectorAll<HTMLTableRowElement>("tr[data-line]")) {
    const rowSide = row.getAttribute("data-side");

    if (rowSide && rowSide !== range.side) {
      continue;
    }

    const lineCell = row.querySelector<HTMLElement>(`.diff-line-${range.side}-num`);
    if (!lineCell) {
      continue;
    }

    const lineNumber = lineNumberFromText(lineCell.querySelector("[data-line-num]")?.textContent);

    if (!lineNumber || !lineIsInRange(lineNumber, range)) {
      continue;
    }

    addRangeDecoration(lineCell, "peers-comment-range-rail");
  }
}

function highlightUnifiedRange(container: HTMLElement, range: SelectedRange) {
  const lineAttribute = range.side === "old" ? "data-line-old-num" : "data-line-new-num";

  for (const row of container.querySelectorAll<HTMLTableRowElement>("tr[data-line]")) {
    const lineNumber = lineNumberFromText(row.querySelector(`[${lineAttribute}]`)?.textContent);

    if (!lineNumber || !lineIsInRange(lineNumber, range)) {
      continue;
    }

    const lineCell = row.querySelector<HTMLElement>(".diff-line-num");

    if (lineCell) {
      addRangeDecoration(lineCell, "peers-comment-range-rail");
    }
  }
}

function highlightRangeCommentBox(container: HTMLElement, range: SelectedRange) {
  const lineKey = `${range.endLine}-extend`;
  const sideClass = range.side === "old" ? "old" : "new";
  const sideSpecificTargets = container.querySelectorAll<HTMLElement>(
    [
      `tr[data-line="${lineKey}"][data-side="${range.side}"] .diff-line-extend-wrapper`,
      `div[data-line="${lineKey}"][data-side="${range.side}"] .diff-line-extend-wrapper`,
      `.diff-line-extend-wrapper[data-line="${range.endLine}-extend-content"][data-side="${range.side}"]`,
      `tr[data-line="${lineKey}"] .diff-line-extend-${sideClass}-content`,
      `div[data-line="${lineKey}"] .diff-line-extend-${sideClass}-content`,
    ].join(","),
  );

  if (sideSpecificTargets.length > 0) {
    sideSpecificTargets.forEach((target) => addRangeDecoration(target, "peers-comment-range-box"));
    return;
  }

  for (const row of container.querySelectorAll<HTMLElement>(
    `[data-line="${lineKey}"][data-state="extend"]`,
  )) {
    const wrappers = row.querySelectorAll<HTMLElement>(".diff-line-extend-wrapper");

    if (wrappers.length === 1) {
      addRangeDecoration(wrappers[0], "peers-comment-range-box");
    }
  }
}

function addRangeDecoration(element: HTMLElement, className: string) {
  element.classList.add(className);
  element.setAttribute("data-peers-range-decoration", "true");
}

function lineNumberFromText(value: string | null | undefined) {
  const lineNumber = Number(value);
  return Number.isFinite(lineNumber) ? lineNumber : undefined;
}

function lineIsInRange(lineNumber: number, range: SelectedRange) {
  return lineNumber >= range.startLine && lineNumber <= range.endLine;
}

function threadExtendData(threads: CommentThread[], activeCommentId: string | undefined) {
  const extendData: {
    oldFile: Record<string, { data: ThreadExtendData }>;
    newFile: Record<string, { data: ThreadExtendData }>;
  } = {
    oldFile: {},
    newFile: {},
  };

  for (const thread of threads) {
    if (thread.scope !== "line") {
      continue;
    }

    const sideKey = thread.anchor.side === "old" ? "oldFile" : "newFile";
    const lineKey = String(thread.anchor.endLine);
    const existingThreads = extendData[sideKey][lineKey]?.data.threads ?? [];

    extendData[sideKey][lineKey] = {
      data: {
        activeCommentId,
        threads: [...existingThreads, thread],
      },
    };
  }

  return extendData;
}

type InlineThreadStackProps = {
  activeCommentId?: string;
  threads: CommentThread[];
};

function InlineThreadStack({ activeCommentId, threads }: InlineThreadStackProps) {
  const hasRangeThread = threads.some(
    (thread) =>
      thread.scope === "line" &&
      isMultiLineRange({
        endLine: thread.anchor.endLine,
        side: thread.anchor.side,
        startLine: thread.anchor.startLine,
      }),
  );

  return (
    <div
      className={[
        "space-y-3 bg-muted/30 p-3 font-sans",
        hasRangeThread ? "peers-comment-range-content" : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
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

function splitSideToFileSide(side: SplitSide): FileSide {
  return side === SplitSide.old ? "old" : "new";
}

function fileSideToSplitSide(side: FileSide) {
  return side === "old" ? SplitSide.old : SplitSide.new;
}

function languageForPath(path: string) {
  const extension = path.split(".").pop()?.toLowerCase();

  switch (extension) {
    case "cjs":
    case "js":
    case "mjs":
      return "javascript";
    case "css":
      return "css";
    case "html":
      return "xml";
    case "json":
      return "json";
    case "md":
    case "mdx":
      return "markdown";
    case "rs":
      return "rust";
    case "sh":
    case "zsh":
      return "bash";
    case "toml":
      return "ini";
    case "ts":
      return "typescript";
    case "tsx":
      return "tsx";
    case "yml":
    case "yaml":
      return "yaml";
    default:
      return "plaintext";
  }
}
