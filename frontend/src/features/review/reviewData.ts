import { useMemo } from "react";
import { create } from "zustand";

export type FileStatus = "modified" | "added" | "deleted" | "renamed" | "unchanged";

export type ReviewFile = {
  path: string;
  status: FileStatus;
  isChanged: boolean;
  viewed: boolean;
  commentCount: number;
  addedLines: number;
  removedLines: number;
};

export type DiffLine = {
  oldNumber?: number;
  newNumber?: number;
  kind: "context" | "added" | "deleted";
  oldText?: string;
  newText?: string;
};

export type CommentThread = {
  id: string;
  path: string;
  lineLabel: string;
  resolved: boolean;
  comments: Array<{
    id: string;
    authorName: string;
    authorKind: "human" | "agent";
    body: string;
  }>;
};

const reviewFiles: ReviewFile[] = [
  {
    path: "src/cli.rs",
    status: "modified",
    isChanged: true,
    viewed: false,
    commentCount: 2,
    addedLines: 42,
    removedLines: 11,
  },
  {
    path: "src/comments.rs",
    status: "added",
    isChanged: true,
    viewed: false,
    commentCount: 1,
    addedLines: 218,
    removedLines: 0,
  },
  {
    path: "src/review.rs",
    status: "modified",
    isChanged: true,
    viewed: true,
    commentCount: 0,
    addedLines: 67,
    removedLines: 9,
  },
  {
    path: "frontend/src/features/review/QuickAccess.tsx",
    status: "added",
    isChanged: true,
    viewed: false,
    commentCount: 0,
    addedLines: 96,
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
    lineLabel: "src/cli.rs:42-47",
    resolved: false,
    comments: [
      {
        id: "cmt_validation",
        authorName: "Jonas",
        authorKind: "human",
        body: "This command should validate mutually exclusive body inputs before appending an event.",
      },
      {
        id: "cmt_agent_reply",
        authorName: "Codex",
        authorKind: "agent",
        body: "I can move this into a pure helper so the CLI branch stays small.",
      },
    ],
  },
  {
    id: "thr_agent_context",
    path: "src/comments.rs",
    lineLabel: "src/comments.rs:117",
    resolved: false,
    comments: [
      {
        id: "cmt_context",
        authorName: "Jonas",
        authorKind: "human",
        body: "Make sure unresolved comments are easy for agents to scan without the UI.",
      },
    ],
  },
  {
    id: "thr_spec",
    path: "spec.md",
    lineLabel: "spec.md:315",
    resolved: true,
    comments: [
      {
        id: "cmt_spec",
        authorName: "ai agent",
        authorKind: "agent",
        body: "The IO boundary rule is reflected in the storage API shape.",
      },
    ],
  },
];

export const modifiedDiff: DiffLine[] = [
  {
    oldNumber: 38,
    newNumber: 38,
    kind: "context",
    oldText: "match command {",
    newText: "match command {",
  },
  { oldNumber: 39, kind: "deleted", oldText: "  Comment::Add(args) => append_comment(args)," },
  { newNumber: 39, kind: "added", newText: "  Comment::Add(args) => create_thread(args).await?," },
  {
    newNumber: 40,
    kind: "added",
    newText: "  Comment::Reply(args) => reply_to_thread(args).await?,",
  },
  { oldNumber: 40, newNumber: 41, kind: "context", oldText: "}", newText: "}" },
];

export const addedFileLines = [
  "use facet::Facet;",
  "",
  "#[derive(Clone, Debug, Facet, PartialEq)]",
  "pub struct CommentThread {",
  "  pub id: String,",
  "  pub resolved: bool,",
  "}",
];

type ReviewDataState = {
  files: ReviewFile[];
  threads: CommentThread[];
};

const useReviewDataStore = create<ReviewDataState>(() => ({
  files: reviewFiles,
  threads: commentThreads,
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
