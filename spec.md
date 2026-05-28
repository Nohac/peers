# Committeer Spec

Committeer is a local Git review tool. It provides a GitHub-like review UI for local changes and branch reviews, while storing comments in the project so humans and AI agents can read, create, and respond to review feedback.

## Goals

- Review unstaged, staged, full working tree, and branch-range diffs locally.
- Select one or more diff/file lines and create comment threads.
- Let humans and AI agents add, edit, delete, reply to, resolve, and reopen comments.
- Store review data in a local project folder with an append-friendly format.
- Use Rust for the backend, gitoxide for Git access, Vox for RPC, facet for serialization, and TanStack Start for the frontend.
- Keep the implementation simple: single Rust crate, minimal tests, behavior-oriented code organization.

## Non-Goals

- No hosted service.
- No login or account system.
- No multi-user permissions model.
- No database.
- No rich text editor in the first pass.
- No code search in the first pass.

## Technology

Backend:

- Rust
- Single crate, no workspace initially
- Tokio runtime
- Async APIs by default, including filesystem operations
- Clap derive for CLI parsing
- gitoxide / `gix` for Git access
- Vox for local RPC
- `facet` and `facet-json` for serialization
- Arborium for server-side syntax highlighting

Frontend:

- TanStack Start
- React Query
- shadcn/ui components
- Tailwind CSS
- lucide-react icons

The current frontend scaffold has TanStack Start SPA mode enabled. SSR mode and future Rust-binary bundling need to be handled explicitly.

## Project Shape

Single Rust crate:

```text
committeer/
  Cargo.toml
  src/
    main.rs
    cli.rs
    diff.rs
    review.rs
    comments.rs
    server.rs
    rpc.rs
    ui_assets.rs
  frontend/
```

Do not organize by generic category names like `types.rs`. Put data structures next to the behavior that owns them.

Suggested backend ownership:

- `cli.rs`: command parsing and command dispatch.
- `diff.rs`: review target resolution, gitoxide diff loading, diff normalization, highlighting integration.
- `review.rs`: review creation, review metadata, review lifecycle, current review selection.
- `comments.rs`: event model, JSONL parsing/encoding, replay, comment commands, agent context rendering.
- `rpc.rs`: Vox service trait and RPC-specific request/response DTOs.
- `server.rs`: local HTTP server, Vox endpoint, one-use token, static/frontend serving.
- `ui_assets.rs`: embedded frontend assets when packaging is added.

Frontend:

```text
frontend/src/features/
  review/
    ReviewWorkspace.tsx
    FileSidebar.tsx
    QuickAccess.tsx
    quickAccessSearch.ts
    reviewQueries.ts
  diff/
    DiffViewer.tsx
    DiffFile.tsx
    DiffLine.tsx
    rangeSelection.ts
  comments/
    CommentThread.tsx
    CommentComposer.tsx
    commentAnchors.ts
```

Do not create broad `types.ts` files. Keep types close to the component or logic that uses them, unless they are genuinely shared across features.

Frontend file structure rules:

- Avoid large files.
- Prefer one meaningful component per file.
- Split large review surfaces into smaller components before they become hard to scan.
- Route files and other compositional files should stay lean.
- Compositional files should mostly arrange smaller components and pass data; they should contain minimal Tailwind.
- Primitive component files may use as much inline Tailwind as needed.
- Primitive components should follow the same structure and conventions as shadcn components.

## CLI

Use Clap derive for CLI parsing.

Primary commands:

```bash
committeer diff
committeer diff --cached
committeer diff --all
committeer review
committeer review --base main
committeer review --base origin/main
```

Review creation:

```bash
committeer review create --kind working-tree
committeer review create --kind cached
committeer review create --base main --head HEAD
committeer review list
committeer review current
```

Comment commands:

```bash
committeer comment add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body "This bypasses validation."

committeer comment add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body-file -

committeer comment reply thr_123 --body "I fixed this."
committeer comment reply thr_123 --body-file -
committeer comment edit cmt_123 --body "Updated comment."
committeer comment delete cmt_123
committeer comment resolve thr_123
committeer comment reopen thr_123
```

Agent support:

```bash
committeer --agent comment add ...
committeer --author-kind agent --author-name Codex comment reply ...
committeer agent-context
committeer agent-context --review rev_123
```

Environment overrides:

```bash
COMMITTEER_AUTHOR_KIND=agent
COMMITTEER_AUTHOR_NAME=Codex
```

## Review Modes

`committeer diff`:

- Reviews unstaged changes.
- Equivalent intent to `git diff`.

`committeer diff --cached`:

- Reviews staged changes.
- Equivalent intent to `git diff --cached`.

`committeer diff --all`:

- Reviews all current changes from `HEAD` to working tree, including staged and unstaged changes.

`committeer review`:

- Reviews the current branch against `main` by default.
- Uses merge-base of base branch and `HEAD`.

`committeer review --base <rev>`:

- Reviews merge-base of `<rev>` and `HEAD` to `HEAD`.

Explicit range:

- Future-compatible with `--base <rev> --head <rev>`.

## Runtime

When opening the UI:

- Discover the Git repo root.
- Create or load a review session.
- Start a localhost server on `127.0.0.1:<random-port>`.
- Generate a one-use token.
- Open the review UI URL.
- Keep the process running until browser/session exit or Ctrl-C.

The server must not listen publicly by default.

## Identity

There is no login.

Default human identity comes from Git config:

```bash
git config user.name
git config user.email
```

Author model:

```rust
Author {
    kind: AuthorKind,
    display_name: String,
    email: Option<String>,
}
```

Author kinds:

- `human`
- `agent`

If an agent identity is not specified, use `ai agent`.

Identity is descriptive, not a security boundary. Edit/delete ownership is soft and local.

## Storage

Review data is stored inside the reviewed project:

```text
.committeer/
  current
  reviews/
    rev_20260528_121530_a1b2c3/
      review.md
      events.jsonl
      agent-context.md
```

`events.jsonl` is canonical.

`review.md` is generated for humans.

`agent-context.md` is generated for agents and should contain unresolved comments with enough file, line, and surrounding context to act on them.

Use append-only JSONL events so agents can append safely and merge conflicts stay manageable.

Example events:

```json
{"kind":"review_created","review_id":"rev_20260528_121530_a1b2c3","created_at":"2026-05-28T12:15:30Z","target":{"kind":"branch","base":"main","head":"HEAD"}}
{"kind":"thread_created","thread_id":"thr_01j","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"},"anchor":{"path":"src/foo.rs","side":"new","start_line":42,"end_line":47,"content_hash":"..."},"body":"This bypasses validation."}
{"kind":"comment_added","thread_id":"thr_01j","comment_id":"cmt_01j","author":{"kind":"agent","display_name":"Codex","email":null},"body":"I can fix this by moving validation before the write."}
{"kind":"thread_resolved","thread_id":"thr_01j","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"}}
```

Event kinds:

- `review_created`
- `review_metadata_updated`
- `thread_created`
- `comment_added`
- `comment_edited`
- `comment_deleted`
- `thread_resolved`
- `thread_reopened`
- `thread_anchored`
- `file_marked_viewed`
- `review_submitted`

Derived state is rebuilt by replaying events.

## IO Boundary Rule

Keep filesystem/path code as a thin shell. Any meaningful behavior should operate on loaded data, buffers, readers, writers, cursors, or strings.

Use Tokio filesystem APIs for real IO.

Preferred:

```rust
async fn parse_events(input: &str) -> Result<Vec<ReviewEvent>>;
async fn parse_events_from_reader(reader: impl AsyncBufRead + Unpin) -> Result<Vec<ReviewEvent>>;
fn encode_event(event: &ReviewEvent) -> Result<String>;
fn replay_events(events: &[ReviewEvent]) -> ReviewState;
async fn render_agent_context(state: &ReviewState, out: impl AsyncWrite + Unpin) -> Result<()>;
```

Thin untested wrappers:

```rust
async fn load_events_file(path: &Path) -> Result<Vec<ReviewEvent>>;
async fn append_event_file(path: &Path, event: &ReviewEvent) -> Result<()>;
```

Avoid large functions that take `&Path` and perform parsing, validation, replay, or transformation internally.

The same principle applies to gitoxide:

- Keep repository access thin.
- Normalize and transform already-loaded diff data in separate functions.

## Async and Concurrency

Use async consistently in the backend.

- Use Tokio as the runtime.
- Use async file IO through `tokio::fs` and Tokio readers/writers.
- Keep sync CPU-only functions sync when they do not perform IO.
- Avoid blocking operations inside async request handlers.

Avoid shared mutable state by default.

- Minimize use of `Arc`, `Mutex`, and `RwLock`.
- Avoid `Arc<Mutex<_>>` especially.
- Prefer ownership, message flow, immutable snapshots, request-local state, or append-only storage.
- If concurrent mutable maps are genuinely needed, prefer purpose-built concurrent structures such as `DashMap`.
- Keep shared server state small and explicit.

Avoid spawning Tokio tasks unless there is a clear lifecycle reason.

- Prefer local async blocks:

```rust
let load_review = async { /* ... */ };
let load_diff = async { /* ... */ };
```

- Combine concurrent work with `tokio::select!`, `tokio::join!`, `futures::future::join_all`, or similar helpers.
- Spawn only for background work that must outlive the current request or needs independent cancellation/lifecycle handling.

## Diff Model

Core concepts:

- `ReviewTarget`
- `ReviewSession`
- `ChangedFile`
- `ReviewableFile`
- `FileDiff`
- `Hunk`
- `DiffLine`
- `LineAnchor`
- `CommentThread`
- `Comment`

Changed file statuses:

- modified
- added
- deleted
- renamed
- binary

Rendering rules:

- Modified files use side-by-side diff by default.
- Renamed files with edits use side-by-side diff by default.
- Added files use full-width file view, not side-by-side.
- Deleted files use full-width old-file view.
- Unchanged files use full-width file view.
- Binary files show metadata and support file-level comments.

Every changed file should also allow opening a full-file view.

Full-file view:

- Shows the whole relevant file.
- Allows comments on any line.
- For modified files, annotates changed lines.
- Uses current/new file content by default.

## Anchors

Thread anchors must survive reasonable file edits.

Store:

- path
- old path, if renamed
- side: `old` or `new`
- start line
- end line
- hunk header, when applicable
- selected text hash
- nearby context hash
- base/head object IDs when known

Refresh relocation order:

1. Exact path, side, and line range.
2. Same path and selected text hash.
3. Nearby context hash.
4. Same file fallback marked outdated.
5. Detached unresolved thread if no match exists.

Anchor relocation is one of the few pieces that should have unit tests.

## Highlighting

Use Arborium in the backend.

The backend should return highlighted line fragments or safe HTML, plus separate diff metadata.

Do not bake diff colors into highlighted code. Syntax highlighting and diff state should remain separate:

- syntax spans come from Arborium
- added/deleted/context state comes from diff metadata
- selected/commented states come from frontend CSS

## RPC

Use Vox over local WebSocket.

Suggested service shape:

```rust
#[vox::service]
pub trait ReviewApi {
    async fn get_review(&self, review_id: ReviewId) -> Result<ReviewSession, ReviewError>;
    async fn list_files(&self, review_id: ReviewId, include_unchanged: bool) -> Result<Vec<ReviewableFile>, ReviewError>;
    async fn get_file_diff(&self, review_id: ReviewId, path: RepoPath) -> Result<FileDiff, ReviewError>;
    async fn get_full_file(&self, review_id: ReviewId, path: RepoPath, side: FileSide) -> Result<FullFileView, ReviewError>;
    async fn create_thread(&self, input: CreateThreadInput) -> Result<CommentThread, ReviewError>;
    async fn reply_to_thread(&self, input: ReplyInput) -> Result<Comment, ReviewError>;
    async fn edit_comment(&self, input: EditCommentInput) -> Result<Comment, ReviewError>;
    async fn delete_comment(&self, input: DeleteCommentInput) -> Result<(), ReviewError>;
    async fn resolve_thread(&self, thread_id: ThreadId) -> Result<(), ReviewError>;
    async fn reopen_thread(&self, thread_id: ThreadId) -> Result<(), ReviewError>;
    async fn mark_file_viewed(&self, input: MarkFileViewedInput) -> Result<(), ReviewError>;
    async fn refresh_diff(&self, review_id: ReviewId) -> Result<ReviewSession, ReviewError>;
}
```

Add live update channels later if needed.

## Frontend Layout

The review UI should feel close to GitHub's pull request review experience.

Desktop layout:

```text
top bar: review target, refresh, view mode, unresolved count
left: file sidebar
center: diff or full-file viewer
right: comments panel
```

Use shadcn primitives:

- `Resizable` for panes
- `Sidebar` for file list
- `ScrollArea` for long lists and diffs
- `Popover` for inline comment composer
- `Sheet` for comments on narrow screens
- `Textarea` for Markdown comments
- `Badge`, `Button`, `Tabs`, `ToggleGroup`, `Tooltip`, `Separator`

The first screen is the review workspace, not a landing page.

Compositional layout files should stay lean. For example, `ReviewWorkspace.tsx` should compose the toolbar, sidebar, diff viewer, comments panel, and quick access menu, but detailed row styling and primitive UI behavior should live in smaller component files.

## File Sidebar

The left sidebar should always be available on desktop.

Default:

- Show only changed files.
- Group by status.
- Show file status badge.
- Show viewed state.
- Show unresolved comment count.

Toggle:

```text
Show unchanged files
```

When enabled:

- Include unchanged reviewable files.
- Opening an unchanged file shows full-file view.
- Users can comment on unchanged lines.

This is useful for comments like "this nearby code should also change."

## Line Selection

GitHub-like behavior:

1. Hovering a line gutter reveals a comment-plus affordance.
2. Clicking starts a single-line selection.
3. Dragging or shift-clicking selects a range.
4. A floating comment button appears near the selected range.
5. Clicking opens a comment composer.
6. Submitting creates a thread anchored to that range.

Use a lucide icon such as `MessageSquarePlus`.

Selection model:

```ts
type SelectedRange = {
  filePath: string
  side: "old" | "new"
  startLine: number
  endLine: number
  view: "diff" | "full-file"
}
```

Do not allow one selection to span both old and new sides in the first pass.

## Comments

Supported operations:

- Add thread.
- Reply to thread.
- Edit comment.
- Delete comment.
- Resolve thread.
- Reopen thread.

Threads render:

- Inline below the selected line/range.
- In the right comments panel.
- As counts in the file sidebar.

Comments use plain Markdown text in a textarea.

No rich text editor initially.

## Quick Access Menu

Provide a custom quick access menu. Do not use `cmdk`.

Shortcut:

```text
Cmd+K / Ctrl+K
```

First pass search scopes:

- Files.
- Comments.

Future scope:

- Code search.
- Actions.
- Review sessions.

The file search must respect the sidebar's "show unchanged files" toggle:

- Toggle off: search changed files only.
- Toggle on: search changed and unchanged reviewable files.

Comment search should search all comments in the current review, regardless of the file visibility filter. Selecting a comment in a currently hidden unchanged file should open the file directly and indicate that it is outside the current file filter.

Result model:

```ts
type QuickAccessResult =
  | {
      kind: "file"
      path: string
      status?: FileStatus
      isChanged: boolean
      commentCount: number
    }
  | {
      kind: "comment"
      threadId: string
      commentId: string
      path: string
      lineLabel: string
      authorName: string
      excerpt: string
      resolved: boolean
    }
```

Search implementation:

- Keep it local and simple initially.
- Case-insensitive substring matching.
- Boost basename matches over directory matches.
- Boost prefix matches over contains matches.
- Search comment body and author display name.

Keep search logic pure:

```ts
function buildQuickAccessResults(input: {
  query: string
  files: ReviewableFile[]
  threads: CommentThread[]
  includeUnchangedFiles: boolean
}): QuickAccessResult[]
```

### Quick Access Tailwind Layout

The search input must be pinned to a stable screen position. It must not move when result height changes.

Use Tailwind classes rather than handwritten CSS where practical:

```tsx
<div className="fixed inset-0 z-50 bg-background/70 backdrop-blur-sm">
  <div className="fixed left-1/2 top-[12vh] grid max-h-[min(720px,76vh)] w-[min(760px,calc(100vw-2rem))] -translate-x-1/2 grid-rows-[auto_minmax(0,1fr)] overflow-hidden rounded-lg border bg-background shadow-lg">
    <div className="sticky top-0 z-10 border-b bg-background p-3">
      {/* input and filter chips */}
    </div>
    <div className="min-h-0 overflow-auto">
      {/* results or empty state */}
    </div>
  </div>
</div>
```

The empty state belongs inside the scrollable results area so the panel geometry remains stable.

## Visual Design

- Quiet, dense, work-focused UI.
- Prefer familiar review-tool patterns over decorative UI.
- No landing page.
- No oversized hero sections.
- No decorative gradient backgrounds or orbs.
- Text must fit within controls at desktop and mobile sizes.
- Use icons for tool buttons where appropriate.
- Use Tailwind and shadcn design tokens.
- Only use colors provided by the shadcn theme.
- Add new theme colors only when there is a concrete repeated semantic need.
- Do not introduce one-off hardcoded colors in component Tailwind classes.

## Frontend Tooling

Use the configured frontend tools in `frontend/package.json`:

```bash
bun run fmt
bun run fmt:check
bun run lint
bun run lint:fix
bun run ts:check
```

- Use `oxfmt` as the frontend formatter.
- Use `oxlint` as the frontend linter.
- Use `tsgo --noEmit` for TypeScript checking.
- Keep generated and hand-written frontend code passing all three checks before considering frontend work complete.
- Prefer fixing lint and type errors in the source instead of suppressing them.

## Testing Policy

Keep testing minimal, but keep code testable.

Test only logic that can become subtle:

- JSONL event parse/encode roundtrip if custom logic exists.
- Event replay.
- Anchor relocation.
- Agent context rendering if formatting becomes non-trivial.
- Diff normalization only if it gains meaningful branching complexity.

Do not test:

- Filesystem wrappers.
- Path construction wrappers.
- Simple DTO mappings.
- CLI flag plumbing unless it becomes subtle.
- Basic component wrappers.

## Implementation Order

1. Add single Rust crate structure.
2. Implement CLI skeleton.
3. Implement review storage with `events.jsonl`.
4. Implement author detection from Git config and CLI/env overrides.
5. Implement comment add/reply/edit/delete/resolve/reopen commands.
6. Implement generated `agent-context.md`.
7. Implement gitoxide branch review diff: `review --base main`.
8. Implement `diff`, `diff --cached`, and `diff --all`.
9. Add Arborium highlighting.
10. Add Vox RPC service.
11. Build TanStack review workspace with file sidebar and diff viewer.
12. Add line/range selection and inline comment composer.
13. Add comment panel and sidebar counts.
14. Add full-file view and unchanged-file toggle.
15. Add custom quick access menu.
16. Add packaging path for embedded frontend assets.
