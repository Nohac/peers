import { useState, type MouseEvent } from "react";
import { format, formatDistanceToNow } from "date-fns";
import { Bot, CheckCircle2, MessageSquareText, Pencil, Save, Trash2, X } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "#/components/ui/alert-dialog.tsx";
import { useReviewCommentActions, type CommentThread } from "../review/reviewData";
import { CommentComposer } from "./CommentComposer";

type InlineCommentThreadProps = {
  thread: CommentThread;
  active?: boolean;
};

type PendingConfirmation = {
  title: string;
  description: string;
  actionLabel: string;
  onConfirm: () => void;
};

export function InlineCommentThread({ thread, active = false }: InlineCommentThreadProps) {
  const { deleteComment, deleteThread, editComment, replyToThread, toggleThreadResolved } =
    useReviewCommentActions();
  const [editingCommentId, setEditingCommentId] = useState<string | null>(null);
  const [draftBody, setDraftBody] = useState("");
  const [replying, setReplying] = useState(false);
  const [pendingConfirmation, setPendingConfirmation] = useState<PendingConfirmation | null>(null);

  function beginEdit(commentId: string, body: string) {
    setEditingCommentId(commentId);
    setDraftBody(body);
  }

  function cancelEdit() {
    setEditingCommentId(null);
    setDraftBody("");
  }

  function commitEdit(commentId: string) {
    const body = draftBody.trim();
    if (body.length === 0) {
      return;
    }
    editComment(thread.id, commentId, body);
    cancelEdit();
  }

  function commitDeleteComment(commentId: string) {
    deleteComment(thread.id, commentId);
  }

  function commitReply(body: string) {
    replyToThread(thread.id, body);
    setReplying(false);
  }

  function removeThread(event: MouseEvent) {
    const invalidatesActivity = thread.comments.length > 1 || thread.resolved;
    if (!invalidatesActivity) {
      event.preventDefault();
      deleteThread(thread.id);
      return;
    }

    setPendingConfirmation({
      actionLabel: "Delete thread",
      description: threadDeleteWarning,
      onConfirm: () => deleteThread(thread.id),
      title: "Delete thread?",
    });
  }

  function onPotentiallyInvalidatingAction(
    event: MouseEvent,
    input: PendingConfirmation & { commentId: string },
  ) {
    const commentIndex = thread.comments.findIndex((comment) => comment.id === input.commentId);
    if (commentIndex === -1) {
      event.preventDefault();
      return;
    }

    const hasLaterActivity = commentIndex < thread.comments.length - 1 || thread.resolved;
    if (!hasLaterActivity) {
      event.preventDefault();
      input.onConfirm();
      return;
    }

    setPendingConfirmation({
      actionLabel: input.actionLabel,
      description: input.description,
      onConfirm: input.onConfirm,
      title: input.title,
    });
  }

  const rootComment = thread.comments[0];

  return (
    <AlertDialog>
      <section
        className={[
          "rounded-md border bg-card text-card-foreground shadow-sm",
          active ? "ring-2 ring-ring" : "",
        ].join(" ")}
        id={`comment-${thread.id}`}
      >
        <div className="flex items-center justify-between gap-2 border-b px-3 py-2">
          <div className="min-w-0 truncate font-mono text-xs font-medium">{thread.lineLabel}</div>
          <div className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">
            {thread.resolved ? (
              <>
                <CheckCircle2 className="size-4" />
                <span>Resolved</span>
              </>
            ) : (
              <>
                <MessageSquareText className="size-4" />
                <span>Open</span>
              </>
            )}
          </div>
        </div>
        <div className="divide-y">
          {thread.comments.map((comment) => {
            const editing = editingCommentId === comment.id;

            return (
              <article className="p-3 text-sm" key={comment.id}>
                <div className="mb-2 flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex min-w-0 items-center gap-2">
                      <span className="truncate font-medium text-foreground">
                        {comment.authorName}
                      </span>
                      {comment.authorKind === "agent" ? (
                        <Bot className="size-3.5 shrink-0 text-muted-foreground" />
                      ) : null}
                    </div>
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <time title={formatExactTime(comment.createdAt)}>
                        {formatRelativeTime(comment.createdAt)}
                      </time>
                      {comment.editedAt ? (
                        <>
                          <span>edited</span>
                          <time title={formatExactTime(comment.editedAt)}>
                            {formatRelativeTime(comment.editedAt)}
                          </time>
                        </>
                      ) : null}
                    </div>
                  </div>
                  {comment.canEdit ? (
                    <div className="flex shrink-0 items-center gap-1">
                      {editing ? (
                        <>
                          <AlertDialogTrigger asChild>
                            <button
                              className="inline-flex size-7 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                              onClick={(event) =>
                                onPotentiallyInvalidatingAction(event, {
                                  actionLabel: "Save edit",
                                  commentId: comment.id,
                                  description:
                                    "Editing this comment will remove later replies and thread status changes from the visible review state.",
                                  onConfirm: () => commitEdit(comment.id),
                                  title: "Edit comment?",
                                })
                              }
                              title="Save"
                              type="button"
                            >
                              <Save className="size-3.5" />
                            </button>
                          </AlertDialogTrigger>
                          <button
                            className="inline-flex size-7 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                            onClick={cancelEdit}
                            title="Cancel"
                            type="button"
                          >
                            <X className="size-3.5" />
                          </button>
                        </>
                      ) : (
                        <>
                          <button
                            className="inline-flex size-7 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-accent-foreground"
                            onClick={() => beginEdit(comment.id, comment.body)}
                            title="Edit"
                            type="button"
                          >
                            <Pencil className="size-3.5" />
                          </button>
                          <AlertDialogTrigger asChild>
                            <button
                              className="inline-flex size-7 items-center justify-center rounded-md border text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                              onClick={(event) =>
                                onPotentiallyInvalidatingAction(event, {
                                  actionLabel: "Delete comment",
                                  commentId: comment.id,
                                  description:
                                    "Deleting this comment will remove later replies and thread status changes from the visible review state.",
                                  onConfirm: () => commitDeleteComment(comment.id),
                                  title: "Delete comment?",
                                })
                              }
                              title="Delete"
                              type="button"
                            >
                              <Trash2 className="size-3.5" />
                            </button>
                          </AlertDialogTrigger>
                        </>
                      )}
                    </div>
                  ) : null}
                </div>
                {editing ? (
                  <textarea
                    className="min-h-24 w-full resize-y rounded-md border bg-background p-2 text-sm leading-6 outline-none focus:ring-2 focus:ring-ring"
                    onChange={(event) => setDraftBody(event.target.value)}
                    value={draftBody}
                  />
                ) : (
                  <p className="whitespace-pre-wrap leading-6">{comment.body}</p>
                )}
              </article>
            );
          })}
        </div>
        {replying ? (
          <div className="border-t bg-muted/20 p-3">
            <CommentComposer
              autoFocus
              onCancel={() => setReplying(false)}
              onSubmit={commitReply}
              placeholder="Reply to this thread"
              submitLabel="Reply"
            />
          </div>
        ) : null}
        <div className="flex items-center justify-between gap-2 border-t bg-muted/20 px-3 py-2">
          <div className="flex gap-2">
            <button
              className="h-8 rounded-md border px-3 text-xs hover:bg-accent"
              onClick={() => setReplying((open) => !open)}
              type="button"
            >
              Reply
            </button>
            <button
              className="h-8 rounded-md border px-3 text-xs hover:bg-accent"
              onClick={() => toggleThreadResolved(thread.id)}
              type="button"
            >
              {thread.resolved ? "Reopen" : "Resolve"}
            </button>
          </div>
          {rootComment?.canEdit ? (
            <AlertDialogTrigger asChild>
              <button
                className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                onClick={removeThread}
                type="button"
              >
                <Trash2 className="size-3.5" />
                Delete thread
              </button>
            </AlertDialogTrigger>
          ) : null}
        </div>
      </section>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{pendingConfirmation?.title}</AlertDialogTitle>
          <AlertDialogDescription>{pendingConfirmation?.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={() => setPendingConfirmation(null)}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() => {
              pendingConfirmation?.onConfirm();
              setPendingConfirmation(null);
            }}
            variant="destructive"
          >
            {pendingConfirmation?.actionLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

const threadDeleteWarning =
  "Deleting this thread will remove its replies and thread status changes from the visible review state.";

function formatRelativeTime(input: string) {
  return `${formatDistanceToNow(input, { addSuffix: true })}`;
}

function formatExactTime(input: string) {
  return format(input, "PP p");
}
