import { useMemo } from "react";
import { create } from "zustand";

export type FileStatus = "modified" | "added" | "deleted" | "renamed" | "unchanged";
export type FileSide = "old" | "new";

export type ReviewFile = {
  path: string;
  status: FileStatus;
  isChanged: boolean;
  viewed: boolean;
  commentCount: number;
  addedLines: number;
  removedLines: number;
};

export type LineRange = {
  start: number;
  end: number;
};

export type DiffSection =
  | {
      context: {
        old: LineRange;
        new: LineRange;
      };
    }
  | {
      added: {
        new: LineRange;
      };
    }
  | {
      removed: {
        old: LineRange;
      };
    };

export type DiffRow = {
  oldNumber?: number;
  newNumber?: number;
  tone: "context" | "added" | "deleted";
  oldText?: string;
  newText?: string;
};

export type DiffHunk = {
  old?: LineRange;
  new?: LineRange;
  sections: DiffSection[];
};

export type FileDiff = {
  path: string;
  hunks: DiffHunk[];
};

export type FileContent = {
  old?: string[];
  new?: string[];
};

type ReviewPayload = {
  files: ReviewFile[];
  fileContentsByPath: Record<string, FileContent>;
  fileDiffsByPath: Record<string, FileDiff>;
  threads: CommentThread[];
};

export type ReviewComment = {
  id: string;
  authorName: string;
  authorKind: "human" | "agent";
  body: string;
  createdAt: string;
  editedAt?: string;
  canEdit: boolean;
};

export type CommentThread = {
  id: string;
  path: string;
  lineLabel: string;
  anchor: {
    side: "old" | "new";
    startLine: number;
    endLine: number;
  };
  resolved: boolean;
  comments: ReviewComment[];
};

function sourceLines(source: string) {
  return source.replace(/^\n/, "").replace(/\n$/, "").split("\n");
}

const cliOldLines = sourceLines(`
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::AsyncReadExt;

use crate::comments::{AuthorKind, ReviewEvent, hash_text};
use crate::diff::{FileSide, LineAnchor, ReviewTarget};
use crate::review::{
    AuthorOverride, append_review_event, create_review, current_review_id, discover_repo,
    list_reviews, load_review_state, new_comment_id, new_thread_id, now_rfc3339, review_paths,
};

#[derive(Parser)]
#[command(name = "peers")]
#[command(about = "Local Git review tool")]
pub struct Cli {
    #[arg(long)]
    agent: bool,
    #[arg(long, value_enum)]
    author_kind: Option<AuthorKindArg>,
    #[arg(long)]
    author_name: Option<String>,
    #[arg(long)]
    author_email: Option<String>,
    #[command(subcommand)]
    command: Command,
}

enum Command {
    Diff(DiffArgs),
    Review(ReviewArgs),
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
}
match command {
  Comment::Add(args) => append_comment(args),
}
`);

const cliNewLines = sourceLines(`
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::io::AsyncReadExt;

use crate::comments::{AuthorKind, ReviewEvent, hash_text};
use crate::diff::{FileSide, LineAnchor, ReviewTarget};
use crate::review::{
    AuthorOverride, append_review_event, create_review, current_review_id, discover_repo,
    list_reviews, load_review_state, new_comment_id, new_thread_id, now_rfc3339, review_paths,
};

#[derive(Parser)]
#[command(name = "peers")]
#[command(about = "Local Git review tool")]
pub struct Cli {
    #[arg(long)]
    agent: bool,
    #[arg(long, value_enum)]
    author_kind: Option<AuthorKindArg>,
    #[arg(long)]
    author_name: Option<String>,
    #[arg(long)]
    author_email: Option<String>,
    #[command(subcommand)]
    command: Command,
}

enum Command {
    Diff(DiffArgs),
    Review(ReviewArgs),
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
}
match command {
  Comment::Add(args) => create_thread(args).await?,
  Comment::Reply(args) => reply_to_thread(args).await?,
}
`);

const commentsLines = sourceLines(`
use facet::Facet;

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct CommentThread {
  pub id: String,
  pub resolved: bool,
}
`);

const reviewOldLines = sourceLines(`
use anyhow::Result;

use crate::comments::CommentThread;
use crate::diff::ReviewTarget;

pub struct ReviewSession {
    pub id: String,
    pub target: ReviewTarget,
    pub threads: Vec<CommentThread>,
}

pub async fn load_review_state(id: &str) -> Result<ReviewSession> {
    read_review_file(id).await
}
`);

const reviewNewLines = sourceLines(`
use anyhow::Result;

use crate::comments::CommentThread;
use crate::diff::ReviewTarget;

pub struct ReviewSession {
    pub id: String,
    pub target: ReviewTarget,
    pub threads: Vec<CommentThread>,
    pub changed_files: Vec<String>,
}

pub async fn load_review_state(id: &str) -> Result<ReviewSession> {
    let mut session = read_review_file(id).await?;
    session.changed_files.sort();
    Ok(session)
}
`);

const quickAccessLines = sourceLines(`
import { Search } from "lucide-react";

export function QuickAccess() {
  return (
    <div className="fixed inset-0 z-50 bg-background/70 backdrop-blur-sm">
      <div className="fixed left-1/2 top-[12vh] w-[min(760px,calc(100vw-2rem))] -translate-x-1/2 rounded-lg border bg-background shadow-lg">
        <div className="flex items-center gap-2 border-b p-3">
          <Search className="size-4 text-muted-foreground" />
          <input className="min-w-0 flex-1 bg-transparent outline-none" />
        </div>
      </div>
    </div>
  );
}
`);

const specLines = sourceLines(`
# Peers Spec

Peers is a local Git review tool.

Slogan:

Local Git peer review for humans and agents.

## Goals

- Review unstaged, staged, full working tree, and branch-range diffs locally.
`);

const fileContentsByPath: Record<string, FileContent> = {
  "src/cli.rs": {
    old: cliOldLines,
    new: cliNewLines,
  },
  "src/comments.rs": {
    new: commentsLines,
  },
  "src/review.rs": {
    old: reviewOldLines,
    new: reviewNewLines,
  },
  "frontend/src/features/review/QuickAccess.tsx": {
    new: quickAccessLines,
  },
  "spec.md": {
    new: specLines,
  },
};

const reviewFiles: ReviewFile[] = [
  {
    path: "src/cli.rs",
    status: "modified",
    isChanged: true,
    viewed: false,
    commentCount: 2,
    addedLines: 2,
    removedLines: 1,
  },
  {
    path: "src/comments.rs",
    status: "added",
    isChanged: true,
    viewed: false,
    commentCount: 1,
    addedLines: commentsLines.length,
    removedLines: 0,
  },
  {
    path: "src/review.rs",
    status: "modified",
    isChanged: true,
    viewed: true,
    commentCount: 0,
    addedLines: 4,
    removedLines: 1,
  },
  {
    path: "frontend/src/features/review/QuickAccess.tsx",
    status: "added",
    isChanged: true,
    viewed: false,
    commentCount: 0,
    addedLines: quickAccessLines.length,
    removedLines: 0,
  },
  {
    path: "spec.md",
    status: "unchanged",
    isChanged: false,
    viewed: false,
    commentCount: 1,
    addedLines: 0,
    removedLines: 0,
  },
];

const commentThreads: CommentThread[] = [
  {
    id: "thr_validation",
    path: "src/cli.rs",
    lineLabel: "src/cli.rs:39-40",
    anchor: {
      side: "new",
      startLine: 39,
      endLine: 40,
    },
    resolved: false,
    comments: [
      {
        id: "cmt_validation",
        authorName: "Jonas",
        authorKind: "human",
        body: "This command should validate mutually exclusive body inputs before appending an event.",
        createdAt: "2026-05-28T18:12:00Z",
        canEdit: true,
      },
      {
        id: "cmt_agent_reply",
        authorName: "Codex",
        authorKind: "agent",
        body: "I can move this into a pure helper so the CLI branch stays small.",
        createdAt: "2026-05-28T18:17:00Z",
        canEdit: false,
      },
    ],
  },
  {
    id: "thr_agent_context",
    path: "src/comments.rs",
    lineLabel: "src/comments.rs:4",
    anchor: {
      side: "new",
      startLine: 4,
      endLine: 4,
    },
    resolved: false,
    comments: [
      {
        id: "cmt_context",
        authorName: "Jonas",
        authorKind: "human",
        body: "Make sure unresolved comments are easy for agents to scan without the UI.",
        createdAt: "2026-05-28T18:34:00Z",
        canEdit: true,
      },
    ],
  },
  {
    id: "thr_spec",
    path: "spec.md",
    lineLabel: "spec.md:4",
    anchor: {
      side: "new",
      startLine: 4,
      endLine: 4,
    },
    resolved: true,
    comments: [
      {
        id: "cmt_spec",
        authorName: "ai agent",
        authorKind: "agent",
        body: "The IO boundary rule is reflected in the storage API shape.",
        createdAt: "2026-05-28T18:45:00Z",
        canEdit: false,
      },
    ],
  },
];

const fileDiffsByPath: Record<string, FileDiff> = {
  "src/cli.rs": {
    path: "src/cli.rs",
    hunks: [
      {
        old: { start: 38, end: 40 },
        new: { start: 38, end: 41 },
        sections: [
          { context: { old: { start: 38, end: 38 }, new: { start: 38, end: 38 } } },
          { removed: { old: { start: 39, end: 39 } } },
          { added: { new: { start: 39, end: 40 } } },
          { context: { old: { start: 40, end: 40 }, new: { start: 41, end: 41 } } },
        ],
      },
    ],
  },
  "src/comments.rs": {
    path: "src/comments.rs",
    hunks: [
      {
        new: { start: 1, end: commentsLines.length },
        sections: [{ added: { new: { start: 1, end: commentsLines.length } } }],
      },
    ],
  },
  "src/review.rs": {
    path: "src/review.rs",
    hunks: [
      {
        old: { start: 6, end: 12 },
        new: { start: 6, end: 15 },
        sections: [
          { context: { old: { start: 6, end: 9 }, new: { start: 6, end: 9 } } },
          { added: { new: { start: 10, end: 10 } } },
          { context: { old: { start: 10, end: 11 }, new: { start: 11, end: 12 } } },
          { removed: { old: { start: 12, end: 12 } } },
          { added: { new: { start: 13, end: 15 } } },
        ],
      },
    ],
  },
  "frontend/src/features/review/QuickAccess.tsx": {
    path: "frontend/src/features/review/QuickAccess.tsx",
    hunks: [
      {
        new: { start: 1, end: quickAccessLines.length },
        sections: [{ added: { new: { start: 1, end: quickAccessLines.length } } }],
      },
    ],
  },
};

const reviewPayload: ReviewPayload = {
  files: reviewFiles,
  fileContentsByPath,
  fileDiffsByPath,
  threads: commentThreads,
};

export function diffForPath(path: string) {
  return reviewPayload.fileDiffsByPath[path];
}

export function diffRowsForPath(path: string) {
  const content = reviewPayload.fileContentsByPath[path];
  const diff = diffForPath(path);

  if (!content || !diff) {
    return [];
  }

  return diff.hunks.flatMap((hunk) =>
    hunk.sections.flatMap((section) => diffRowsForSection(section, content)),
  );
}

export function diffLinesForPath(path: string) {
  const diffRows = diffRowsForPath(path);

  if (diffRows.length === 0) {
    return fullFileLinesForPath(path);
  }

  return diffRows.flatMap((line) => {
    const lineNumber = line.tone === "deleted" ? line.oldNumber : line.newNumber;
    const text = line.tone === "deleted" ? line.oldText : line.newText;

    if (lineNumber === undefined || text === undefined) {
      return [];
    }

    return {
      lineNumber,
      text,
      tone: line.tone,
    };
  });
}

export function fullFileLinesForPath(path: string, side: FileSide = "new") {
  const content = reviewPayload.fileContentsByPath[path];
  const lines = content?.[side] ?? content?.new ?? content?.old ?? [];
  const diffLineTones = fullFileDiffLineTones(path, side);

  return lines.map((text, index) => ({
    lineNumber: index + 1,
    text,
    tone: diffLineTones.get(index + 1) ?? ("context" as const),
  }));
}

function fullFileDiffLineTones(path: string, side: FileSide) {
  const diff = diffForPath(path);
  const tones = new Map<number, DiffRow["tone"]>();

  for (const hunk of diff?.hunks ?? []) {
    for (const section of hunk.sections) {
      if (side === "new" && "added" in section) {
        setRangeTone(tones, section.added.new, "added");
      }

      if (side === "old" && "removed" in section) {
        setRangeTone(tones, section.removed.old, "deleted");
      }
    }
  }

  return tones;
}

function diffRowsForSection(section: DiffSection, content: FileContent): DiffRow[] {
  if ("context" in section) {
    return pairedRange(section.context.old, section.context.new).flatMap(({ oldLine, newLine }) => {
      const oldText = content.old?.[oldLine - 1];
      const newText = content.new?.[newLine - 1];

      if (oldText === undefined || newText === undefined) {
        return [];
      }

      return {
        oldNumber: oldLine,
        newNumber: newLine,
        tone: "context" as const,
        oldText,
        newText,
      };
    });
  }

  if ("added" in section) {
    return rangeLines(section.added.new).flatMap((newLine) => {
      const newText = content.new?.[newLine - 1];

      if (newText === undefined) {
        return [];
      }

      return {
        newNumber: newLine,
        tone: "added" as const,
        newText,
      };
    });
  }

  return rangeLines(section.removed.old).flatMap((oldLine) => {
    const oldText = content.old?.[oldLine - 1];

    if (oldText === undefined) {
      return [];
    }

    return {
      oldNumber: oldLine,
      tone: "deleted" as const,
      oldText,
    };
  });
}

function pairedRange(oldRange: LineRange, newRange: LineRange) {
  const lineCount = Math.min(rangeLength(oldRange), rangeLength(newRange));

  return Array.from({ length: lineCount }, (_, index) => ({
    oldLine: oldRange.start + index,
    newLine: newRange.start + index,
  }));
}

function rangeLines(range: LineRange) {
  return Array.from({ length: rangeLength(range) }, (_, index) => range.start + index);
}

function rangeLength(range: LineRange) {
  return Math.max(range.end - range.start + 1, 0);
}

function setRangeTone(
  tones: Map<number, DiffRow["tone"]>,
  range: LineRange,
  tone: DiffRow["tone"],
) {
  for (const lineNumber of rangeLines(range)) {
    tones.set(lineNumber, tone);
  }
}

type ReviewDataState = {
  files: ReviewFile[];
  threads: CommentThread[];
  deleteComment: (threadId: string, commentId: string) => void;
  deleteThread: (threadId: string) => void;
  editComment: (threadId: string, commentId: string, body: string) => void;
  toggleThreadResolved: (threadId: string) => void;
};

const useReviewDataStore = create<ReviewDataState>((set) => ({
  files: reviewPayload.files,
  threads: reviewPayload.threads,
  deleteComment: (threadId, commentId) => {
    set((state) => ({
      threads: state.threads.flatMap((thread) => {
        if (thread.id !== threadId) {
          return [thread];
        }

        const commentIndex = thread.comments.findIndex((comment) => comment.id === commentId);
        if (commentIndex === -1 || !thread.comments[commentIndex]?.canEdit) {
          return [thread];
        }

        const comments = thread.comments.slice(0, commentIndex);
        return comments.length === 0 ? [] : [{ ...thread, comments, resolved: false }];
      }),
    }));
  },
  deleteThread: (threadId) => {
    set((state) => ({
      threads: state.threads.filter((thread) => thread.id !== threadId),
    }));
  },
  editComment: (threadId, commentId, body) => {
    const editedAt = new Date().toISOString();
    set((state) => ({
      threads: state.threads.map((thread) => {
        if (thread.id !== threadId) {
          return thread;
        }

        const commentIndex = thread.comments.findIndex((comment) => comment.id === commentId);
        const comment = thread.comments[commentIndex];
        if (!comment?.canEdit) {
          return thread;
        }

        const comments = thread.comments.slice(0, commentIndex + 1);
        comments[commentIndex] = {
          ...comment,
          body,
          editedAt,
        };

        return {
          ...thread,
          comments,
          resolved: false,
        };
      }),
    }));
  },
  toggleThreadResolved: (threadId) => {
    set((state) => ({
      threads: state.threads.map((thread) =>
        thread.id === threadId ? { ...thread, resolved: !thread.resolved } : thread,
      ),
    }));
  },
}));

type UseReviewFilesInput = {
  includeUnchangedFiles: boolean;
};

export function useReviewFiles({ includeUnchangedFiles }: UseReviewFilesInput) {
  const files = useReviewDataStore((state) => state.files);

  return useMemo(
    () => files.filter((file) => includeUnchangedFiles || file.isChanged),
    [files, includeUnchangedFiles],
  );
}

export function useChangedFiles() {
  const files = useReviewDataStore((state) => state.files);

  return useMemo(() => files.filter((file) => file.isChanged), [files]);
}

export function useReviewFile(path: string) {
  const files = useReviewDataStore((state) => state.files);

  return useMemo(
    () => files.find((candidate) => candidate.path === path) ?? files[0],
    [files, path],
  );
}

export function useThreads() {
  return useReviewDataStore((state) => state.threads);
}

export function useThreadsForFile(path: string) {
  const threads = useThreads();

  return useMemo(() => threads.filter((thread) => thread.path === path), [path, threads]);
}

export function useReviewCommentActions() {
  const deleteComment = useReviewDataStore((state) => state.deleteComment);
  const deleteThread = useReviewDataStore((state) => state.deleteThread);
  const editComment = useReviewDataStore((state) => state.editComment);
  const toggleThreadResolved = useReviewDataStore((state) => state.toggleThreadResolved);

  return {
    deleteComment,
    deleteThread,
    editComment,
    toggleThreadResolved,
  };
}
