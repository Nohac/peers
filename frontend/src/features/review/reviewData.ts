import { useMemo } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useSearch } from "@tanstack/react-router";
import {
  connectPeersReview,
  type ApiCommentThread,
  type ApiReviewPayload,
  type DiffSection as WireDiffSection,
  type FileStatus as WireFileStatus,
  type PeersReviewClient,
} from "./peersReviewClient.gen";

export type FileStatus = "modified" | "added" | "deleted" | "renamed" | "unchanged" | "binary";
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
  reviewId: string;
  targetLabel: string;
  isBranchReview: boolean;
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
  scope: "line" | "file" | "review";
  path?: string;
  lineLabel: string;
  anchor: {
    side: "old" | "new";
    startLine: number;
    endLine: number;
  };
  resolved: boolean;
  comments: ReviewComment[];
};

let latestPayload: ReviewPayload = emptyReviewPayload();
const clientPromises = new Map<string, Promise<PeersReviewClient>>();

export function diffForPath(path: string) {
  return latestPayload.fileDiffsByPath[path];
}

export function fileContentForPath(path: string) {
  return latestPayload.fileContentsByPath[path];
}

type UseReviewFilesInput = {
  includeUnchangedFiles: boolean;
};

export function useReviewFiles({ includeUnchangedFiles }: UseReviewFilesInput) {
  const review = useReviewPayload();

  return useMemo(
    () => review.files.filter((file) => includeUnchangedFiles || file.isChanged),
    [review.files, includeUnchangedFiles],
  );
}

export function useChangedFiles() {
  const review = useReviewPayload();

  return useMemo(() => review.files.filter((file) => file.isChanged), [review.files]);
}

export function useReviewFile(path: string) {
  const review = useReviewPayload();

  return useMemo(
    () => review.files.find((candidate) => candidate.path === path) ?? review.files[0] ?? emptyFile,
    [review.files, path],
  );
}

export function useThreads() {
  return useReviewPayload().threads;
}

export function useThreadsForFile(path: string) {
  const threads = useThreads();

  return useMemo(() => threads.filter((thread) => thread.path === path), [path, threads]);
}

export function useReviewCommentActions() {
  const queryClient = useQueryClient();
  const config = usePeersConfig();
  const mutation = useMutation({
    mutationFn: async (operation: CommentOperation) => {
      const requiredConfig = requirePeersConfig(config);
      const client = await peersClient(requiredConfig);
      const result = await runCommentOperation(client, requiredConfig.token, operation);

      if (!result.ok) {
        throw new Error(result.error);
      }

      return adaptPayload(result.value);
    },
    onSuccess: (payload) => {
      latestPayload = payload;
      queryClient.setQueryData(reviewQueryKey(requirePeersConfig(config)), payload);
    },
  });

  return {
    createThread: (input: {
      scope: "line" | "file" | "review";
      path?: string;
      side?: FileSide;
      startLine?: number;
      endLine?: number;
      body: string;
    }) => {
      mutation.mutate({
        kind: "createThread",
        body: input.body,
        endLine: input.endLine,
        path: input.path,
        scope: input.scope,
        side: input.side,
        startLine: input.startLine,
      });
    },
    deleteComment: (_threadId: string, commentId: string) => {
      mutation.mutate({ kind: "deleteComment", commentId });
    },
    deleteThread: (threadId: string) => {
      mutation.mutate({ kind: "deleteThread", threadId });
    },
    editComment: (_threadId: string, commentId: string, body: string) => {
      mutation.mutate({ kind: "editComment", body, commentId });
    },
    refreshDiff: () => {
      mutation.mutate({ kind: "refreshDiff" });
    },
    replyToThread: (threadId: string, body: string) => {
      mutation.mutate({ kind: "replyToThread", body, threadId });
    },
    toggleThreadResolved: (threadId: string) => {
      const thread = latestPayload.threads.find((candidate) => candidate.id === threadId);
      mutation.mutate({
        kind: thread?.resolved ? "reopenThread" : "resolveThread",
        threadId,
      });
    },
  };
}

function useReviewPayload() {
  const config = usePeersConfig();
  const query = useQuery({
    enabled: config !== undefined,
    queryKey: reviewQueryKey(config),
    queryFn: async () => {
      const requiredConfig = requirePeersConfig(config);
      const result = await (await peersClient(requiredConfig)).getReview(requiredConfig.token);

      if (!result.ok) {
        throw new Error(result.error);
      }

      return adaptPayload(result.value);
    },
  });
  const payload = query.data ?? latestPayload;
  latestPayload = payload;

  return payload;
}

type CommentOperation =
  | {
      kind: "createThread";
      scope: "line" | "file" | "review";
      path?: string;
      side?: FileSide;
      startLine?: number;
      endLine?: number;
      body: string;
    }
  | { kind: "deleteComment"; commentId: string }
  | { kind: "deleteThread"; threadId: string }
  | { kind: "editComment"; commentId: string; body: string }
  | { kind: "refreshDiff" }
  | { kind: "replyToThread"; threadId: string; body: string }
  | { kind: "resolveThread"; threadId: string }
  | { kind: "reopenThread"; threadId: string };

async function runCommentOperation(
  client: PeersReviewClient,
  token: string,
  operation: CommentOperation,
) {
  switch (operation.kind) {
    case "createThread":
      return client.createThread(token, {
        body: operation.body,
        end_line: operation.endLine ?? null,
        path: operation.path ?? null,
        scope: operation.scope,
        side:
          operation.side === "old"
            ? { tag: "Old" }
            : operation.side === "new"
              ? { tag: "New" }
              : null,
        start_line: operation.startLine ?? null,
      });
    case "deleteComment":
      return client.deleteComment(token, { comment_id: operation.commentId });
    case "deleteThread":
      return client.deleteThread(token, { thread_id: operation.threadId });
    case "editComment":
      return client.editComment(token, {
        body: operation.body,
        comment_id: operation.commentId,
      });
    case "refreshDiff":
      return client.refreshDiff(token);
    case "replyToThread":
      return client.replyToThread(token, {
        body: operation.body,
        thread_id: operation.threadId,
      });
    case "resolveThread":
      return client.resolveThread(token, { thread_id: operation.threadId });
    case "reopenThread":
      return client.reopenThread(token, { thread_id: operation.threadId });
  }
}

function adaptPayload(payload: ApiReviewPayload): ReviewPayload {
  return {
    reviewId: payload.review_id,
    targetLabel: payload.target_label,
    isBranchReview: payload.is_branch_review,
    files: payload.files.map((file) => ({
      path: file.path,
      status: fileStatus(file.status),
      isChanged: file.is_changed,
      viewed: file.viewed,
      commentCount: file.comment_count,
      addedLines: file.added_lines,
      removedLines: file.removed_lines,
    })),
    fileContentsByPath: mapRecord(payload.file_contents_by_path, (content) => ({
      old: content.old ?? undefined,
      new: content.new ?? undefined,
    })),
    fileDiffsByPath: mapRecord(payload.file_diffs_by_path, (diff) => ({
      path: diff.path,
      hunks: diff.hunks.map((hunk) => ({
        old: hunk.old ?? undefined,
        new: hunk.new ?? undefined,
        sections: hunk.sections.map(adaptDiffSection),
      })),
    })),
    threads: payload.threads.map(adaptThread),
  };
}

function adaptThread(thread: ApiCommentThread): CommentThread {
  const startLine = thread.anchor.start_line ?? 1;
  const endLine = thread.anchor.end_line ?? startLine;

  return {
    id: thread.id,
    scope: threadScope(thread.scope),
    path: thread.path ?? undefined,
    lineLabel: thread.line_label,
    anchor: {
      side: thread.anchor.side === "old" ? "old" : "new",
      startLine,
      endLine,
    },
    resolved: thread.resolved,
    comments: thread.comments.map((comment) => ({
      id: comment.id,
      authorName: comment.author_name,
      authorKind: comment.author_kind === "agent" ? "agent" : "human",
      body: comment.body,
      createdAt: comment.created_at,
      editedAt: comment.edited_at ?? undefined,
      canEdit: comment.can_edit,
    })),
  };
}

function adaptDiffSection(section: WireDiffSection): DiffSection {
  switch (section.tag) {
    case "Context":
      return { context: section.context };
    case "Added":
      return { added: section.added };
    case "Removed":
      return { removed: section.removed };
  }
}

function fileStatus(status: WireFileStatus): FileStatus {
  switch (status.tag) {
    case "Modified":
      return "modified";
    case "Added":
      return "added";
    case "Deleted":
      return "deleted";
    case "Renamed":
      return "renamed";
    case "Unchanged":
      return "unchanged";
    case "Binary":
      return "binary";
  }
}

function threadScope(scope: string): CommentThread["scope"] {
  if (scope === "file" || scope === "review") {
    return scope;
  }
  return "line";
}

function mapRecord<T, U>(map: Map<string, T> | Record<string, T>, transform: (value: T) => U) {
  const entries = map instanceof Map ? map.entries() : Object.entries(map);
  const record: Record<string, U> = {};

  for (const [key, value] of entries) {
    record[key] = transform(value);
  }

  return record;
}

function peersClient(config: PeersConfig) {
  const existingClient = clientPromises.get(config.voxUrl);
  if (existingClient) {
    return existingClient;
  }

  const client = connectPeersReview(config.voxUrl);
  clientPromises.set(config.voxUrl, client);
  return client;
}

type PeersConfig = {
  token: string;
  voxUrl: string;
};

function usePeersConfig(): PeersConfig | undefined {
  const search = useSearch({ from: "__root__" });
  const voxUrl = search.vox;
  const token = search.token;

  if (!voxUrl || !token) {
    return undefined;
  }

  return { token, voxUrl };
}

function requirePeersConfig(config: PeersConfig | undefined) {
  if (!config) {
    throw new Error("Open Peers from the URL printed by `peers diff` or `peers review`.");
  }
  return config;
}

function reviewQueryKey(config: PeersConfig | undefined) {
  return ["review", config?.voxUrl ?? "missing", config?.token ?? "missing"] as const;
}

function emptyReviewPayload(): ReviewPayload {
  return {
    reviewId: "",
    targetLabel: "",
    isBranchReview: false,
    files: [],
    fileContentsByPath: {},
    fileDiffsByPath: {},
    threads: [],
  };
}

const emptyFile: ReviewFile = {
  path: "",
  status: "unchanged",
  isChanged: false,
  viewed: false,
  commentCount: 0,
  addedLines: 0,
  removedLines: 0,
};
