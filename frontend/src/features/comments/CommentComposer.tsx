import { useState } from "react";
import { Send, X } from "lucide-react";

type CommentComposerProps = {
  autoFocus?: boolean;
  placeholder?: string;
  submitLabel?: string;
  onCancel?: () => void;
  onSubmit: (body: string) => void;
};

export function CommentComposer({
  autoFocus = false,
  placeholder = "Leave a comment",
  submitLabel = "Comment",
  onCancel,
  onSubmit,
}: CommentComposerProps) {
  const [body, setBody] = useState("");
  const trimmedBody = body.trim();

  function submit() {
    if (trimmedBody.length === 0) {
      return;
    }
    onSubmit(trimmedBody);
    setBody("");
  }

  return (
    <div className="rounded-md border bg-card p-3 text-card-foreground shadow-sm">
      <textarea
        autoFocus={autoFocus}
        className="min-h-24 w-full resize-y rounded-md border bg-background p-2 text-sm leading-6 outline-none focus:ring-2 focus:ring-ring"
        onChange={(event) => setBody(event.target.value)}
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            submit();
          }
        }}
        placeholder={placeholder}
        value={body}
      />
      <div className="mt-2 flex items-center justify-end gap-2">
        {onCancel ? (
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md border px-3 text-xs text-muted-foreground hover:bg-accent hover:text-accent-foreground"
            onClick={onCancel}
            type="button"
          >
            <X className="size-3.5" />
            Cancel
          </button>
        ) : null}
        <button
          className="inline-flex h-8 items-center gap-2 rounded-md border bg-primary px-3 text-xs text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
          disabled={trimmedBody.length === 0}
          onClick={submit}
          type="button"
        >
          <Send className="size-3.5" />
          {submitLabel}
        </button>
      </div>
    </div>
  );
}
