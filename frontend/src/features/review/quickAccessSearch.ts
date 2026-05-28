import type { CommentThread, ReviewFile } from "./reviewData";

export type QuickAccessResult =
  | {
      kind: "file";
      path: string;
      status?: ReviewFile["status"];
      isChanged: boolean;
      commentCount: number;
      score: number;
    }
  | {
      kind: "comment";
      threadId: string;
      commentId: string;
      path: string;
      isChanged: boolean;
      lineLabel: string;
      authorName: string;
      excerpt: string;
      resolved: boolean;
      score: number;
    };

type BuildQuickAccessResultsInput = {
  query: string;
  files: ReviewFile[];
  threads: CommentThread[];
};

export function buildQuickAccessResults({
  query,
  files,
  threads,
}: BuildQuickAccessResultsInput): QuickAccessResult[] {
  const normalizedQuery = query.trim().toLowerCase();
  const filesByPath = new Map(files.map((file) => [file.path, file]));
  const fileResults = files.flatMap((file) => {
    const score = scorePath(file.path, normalizedQuery, file.isChanged);
    return score === 0
      ? []
      : [
          {
            kind: "file" as const,
            path: file.path,
            status: file.status,
            isChanged: file.isChanged,
            commentCount: file.commentCount,
            score,
          },
        ];
  });
  const commentResults = threads.flatMap((thread) => {
    const file = filesByPath.get(thread.path);

    if (!file) {
      return [];
    }

    return thread.comments.flatMap((comment) => {
      const haystack = `${comment.body} ${comment.authorName}`.toLowerCase();
      const score =
        normalizedQuery === "" || haystack.includes(normalizedQuery)
          ? thread.resolved
            ? 10
            : 20
          : 0;
      return score === 0
        ? []
        : [
            {
              kind: "comment" as const,
              threadId: thread.id,
              commentId: comment.id,
              path: thread.path,
              isChanged: file.isChanged,
              lineLabel: thread.lineLabel,
              authorName: comment.authorName,
              excerpt: comment.body,
              resolved: thread.resolved,
              score,
            },
          ];
    });
  });

  return [...fileResults, ...commentResults].sort((a, b) => b.score - a.score);
}

function scorePath(path: string, query: string, isChanged: boolean) {
  if (query === "") {
    return isChanged ? 30 : 15;
  }

  const lowerPath = path.toLowerCase();
  const basename = lowerPath.split("/").at(-1) ?? lowerPath;
  if (basename.startsWith(query)) {
    return isChanged ? 100 : 80;
  }
  if (lowerPath.startsWith(query)) {
    return isChanged ? 70 : 55;
  }
  if (basename.includes(query)) {
    return isChanged ? 50 : 35;
  }
  if (lowerPath.includes(query)) {
    return isChanged ? 30 : 20;
  }
  return 0;
}
