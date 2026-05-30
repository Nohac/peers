# Peers Spec

Peers is a local Git review tool. It provides a GitHub-like review UI for local changes and branch reviews, while storing comments in the project so humans and AI agents can read, create, and respond to review feedback.

Slogan:

```text
Local Git peer review for humans and agents.
```

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
- `thiserror` for custom domain errors
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
peers/
  Cargo.toml
  src/
    main.rs
    cli.rs
    diff.rs
    review.rs
    comments.rs
    review_provider.rs
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
- `review_provider.rs`: cloneable async review provider used by web RPC, Neovim LSP, and future local clients.
- `rpc.rs`: Vox service trait and token-checking RPC wrapper around the shared review provider.
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
peers diff
peers diff --cached
peers diff --all
peers review
peers review --base main
peers review --base origin/main
```

Review creation:

```bash
peers review create --kind working-tree
peers review create --kind cached
peers review create --base main --head HEAD
peers review list
peers review current
```

Comment commands:

```bash
peers comment add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body "This bypasses validation."

peers comment add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body-file -

peers comment reply thr_123 --body "I fixed this."
peers comment reply thr_123 --body-file -
peers comment edit cmt_123 --body "Updated comment."
peers comment delete cmt_123
peers comment resolve thr_123
peers comment reopen thr_123
```

Agent support:

```bash
peers --agent comment add ...
peers --author-kind agent --author-name Codex comment reply ...
peers agent-context
peers agent-context --review rev_123
```

Neovim session command:

```bash
peers nvim
peers nvim --review rev_123
```

This starts or attaches to the current review session and exposes the local Vox and `peersdiff` LSP endpoints for Neovim.

An active session should publish ephemeral connection information at:

```text
.peers/reviews/<review-id>/session.json
```

The file includes the process id, review id, Vox URL, `peersdiff` LSP URL, frontend URL, token, and start time. It is an attachment hint for local clients, not canonical review state.

Environment overrides:

```bash
PEERS_AUTHOR_KIND=agent
PEERS_AUTHOR_NAME=Codex
```

## Review Modes

`peers diff`:

- Reviews unstaged changes.
- Equivalent intent to `git diff`.

`peers diff --cached`:

- Reviews staged changes.
- Equivalent intent to `git diff --cached`.

`peers diff --all`:

- Reviews all current changes from `HEAD` to working tree, including staged and unstaged changes.

`peers review`:

- Reviews the current branch against `main` by default.
- Uses merge-base of base branch and `HEAD`.

`peers review --base <rev>`:

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
.peers/
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

File-level and review-level comments should also be included in generated review summaries and agent context.

Use append-only JSONL events so agents can append safely and merge conflicts stay manageable.

Example events:

```json
{"kind":"review_created","review_id":"rev_20260528_121530_a1b2c3","created_at":"2026-05-28T12:15:30Z","target":{"kind":"branch","base":"main","head":"HEAD"}}
{"kind":"thread_created","thread_id":"thr_01j","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"},"anchor":{"path":"src/foo.rs","side":"new","start_line":42,"end_line":47,"content_hash":"..."},"body":"This bypasses validation."}
{"kind":"thread_created","thread_id":"thr_02j","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"},"anchor":{"path":"src/foo.rs","scope":"file"},"body":"This file needs a smaller public API before merging."}
{"kind":"thread_created","thread_id":"thr_03j","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"},"anchor":null,"body":"Before merging, let's decide whether this should be split into two commits."}
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
- `DiffSection`
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
- Side-by-side diffs use equal-width old/new panes that fill the available container width. Each pane owns its horizontal overflow, both panes use the larger content width, and pane scroll positions stay synchronized.
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
- Preserves code whitespace exactly in rendered lines.
- Reuses the same line rendering, comment anchoring, inline thread, and line selection logic as the diff view.
- Diff views may show only changed hunks; full-file views must show the complete file content while preserving the same comment behavior.

Frontend review payload shape:

- `files`: ordered list of `ReviewableFile` metadata for the sidebar and file headers.
- `fileContentsByPath`: map keyed by repo path. Each value contains `old` and/or `new` line arrays for full-file rendering.
- `fileDiffsByPath`: map keyed by repo path. Each `FileDiff` contains hunks with optional old/new line ranges and ordered compact sections.
- `threads`: ordered list of `CommentThread` records anchored by path, side, and line/range, with file-level and review-level scopes added later.

Diff hunks must point back into `fileContentsByPath`:

- Ranges are 1-based and inclusive.
- Context sections contain old and new ranges.
- Added sections contain a new range.
- Removed sections contain an old range.
- The UI expands sections into render rows and reads text from `fileContentsByPath`.
- Added files should have only `new` content and added sections.
- Deleted files should have only `old` content and removed sections.
- Unchanged files may omit `FileDiff`; the UI should render full-file content when they are explicitly shown.

## Anchors

Threads may be line/range anchored, file-level, or review-level.

Line/range anchored threads belong to a file, side, and line/range. They are created from the `Files changed` tab and render inline in diff/full-file views.

File-level threads belong to a file, but not to a specific line. They are created from a file header action labeled `Comment on this file`. Use them for feedback about a file as a whole, such as module boundaries, naming, test coverage, generated code, or whether the file should exist.

Review-level threads have no anchor. They are created from the `Conversation` tab and belong to the review as a whole. Use them for general review discussion, merge readiness, commit structure, follow-up tasks, or questions that are not about a specific line.

Line/range anchors must survive reasonable file edits.

Store for line/range anchors:

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

## Realtime Updates

The UI should stay current without manual refresh whenever the local review state changes.

Sources that must update the open UI:

- Local file changes that alter the reviewed diff.
- CLI comment operations, including human and agent `peers comment add/reply/edit/delete/resolve/reopen` commands.
- UI comment operations from another open browser window or tab.
- File viewed/unviewed changes.
- Review submission events.
- Regenerated `review.md` and `agent-context.md` output when their source event log changes.

Implementation expectations:

- Use the existing local Vox WebSocket connection for update notifications.
- Treat `.peers/reviews/<review-id>/events.jsonl` as the canonical review event source.
- Watch the review event log for append changes and notify connected clients.
- Watch relevant Git working tree/index inputs for diff changes and notify connected clients that the diff payload should refresh.
- The frontend should invalidate/refetch the TanStack Query review payload when it receives a review or diff update notification.
- Update notifications may be coarse-grained at first, such as `review_changed` and `diff_changed`; they do not need per-entity patches in the first pass.
- The UI should keep local interaction state where practical, such as open tabs, sidebar collapse state, active file, active comment, and composer draft, while replacing server-owned review data.
- If an update invalidates the currently visible file, comment, or line anchor, the UI should fall back gracefully to the nearest valid review surface instead of crashing.
- Avoid polling as the primary strategy. A small debounce around file watcher bursts is acceptable.

## Neovim Integration

Peers should provide a first-class Neovim review mode for keyboard-driven code review without requiring the user to leave their editor.

The integration should keep one Peers session process per review. The session process owns review state, Git watchers, event log watchers, Vox, and Neovim-facing services. Launching from either side should attach to the same session:

- `peers diff`, `peers review`, and `peers review create` start or select a review session and publish local connection information in the active review's `session.json`.
- `:PeersReview` in Neovim attaches to an existing session for the current repo/review when one exists.
- If no session exists, `:PeersReview` may start the same Peers session process that the CLI would start, then attach to it.
- The web UI and Neovim UI must see the same event log, realtime updates, and generated review artifacts.

The core review operations should live behind one cloneable async review provider. Vox should expose that provider to the web frontend, and the `peersdiff` LSP should call the same provider directly inside the Peers session process. The provider should avoid external shared mutability by default; if live state becomes necessary, prefer an internal event loop, request/response channels, or purpose-built concurrent maps over `Arc<Mutex<_>>`.

The Neovim surface should be one full-focus synthetic review buffer, not a split-based UI:

```vim
buftype=nofile
bufhidden=hide
buflisted=false
swapfile=false
modifiable=false
readonly=true
filetype=peersdiff
```

Expected behavior:

- The review buffer should not appear in normal buffer lists or Telescope buffer pickers that respect `buflisted=false`.
- The review buffer should be easy to reopen with `:PeersReview` and should remain usable through the jumplist where practical.
- The buffer should render files, hunks, added/removed/context lines, inline threads, multiline range markers, file-level comments, review-level conversation entries, and compact unchanged-context placeholders.
- File context labels, including file header rows and editor breadcrumb/document-symbol context, should include compact Git-style status markers such as `[A]`, `[D]`, `[R]`, `[M]`, `[U]`, or `[B]` for added, deleted, renamed, modified, unchanged, and binary files.
- Peers owns the render model. The `peersdiff` LSP exposes a custom `peers/renderReview` request that returns synthetic lines, row metadata, structural highlights, and symbol metadata derived from the shared review provider.
- The Lua attachment layer applies rendered lines and extmarks to the synthetic buffer, while Rust remains the source of truth for row semantics and review data.
- Structural highlights cover file headers, hunk headers, line numbers, add/delete gutter markers, and comment rows. Prefer gutter/prefix color over full-line color unless a stronger visual treatment is required.
- Current-side added and context rows should mirror Tree-sitter capture highlights from hidden real file buffers so the review buffer follows the user's Neovim theme and language setup. This mirroring must be viewport-scoped for performance on large reviews. Deleted/base-side rows may start with structural diff coloring only.
- Do not depend on Neovim folds or persistent split sidebars for the primary workflow.
- The review buffer remains read-only. Comment bodies are entered through a focused writable composer, such as a temporary floating buffer.
- Neovim comment composers should use native editor primitives. The default composer is a writable floating scratch buffer anchored near the relevant review row; `<Enter>` should remain available for newlines, and submission should use an explicit mapping such as `<C-s>`.

Peers should expose a `peersdiff` language server for this synthetic buffer. Use the `tower-lsp-server` crate from `tower-lsp-community/tower-lsp-server`, the maintained community fork of `tower-lsp`, unless a better maintained LSP server crate is chosen before implementation.

The `peersdiff` LSP should let users keep their existing LSP mappings instead of defining Peers-specific replacements:

- `textDocument/hover`: show Peers metadata for review rows and proxy source hover for mapped current-side code rows.
- `textDocument/definition`: proxy from mapped current-side code rows to the hidden real source buffer's attached language servers and open the real target in the current window.
- `textDocument/references`: proxy references from mapped current-side code rows where practical, using the user's normal quickfix/location-list behavior.
- `textDocument/codeAction`: expose context-aware review actions only. Source LSP code actions from hidden buffers should not be proxied into the review buffer. Source rows should offer labels such as `Add line comment` or `Add comment on lines 3..9`; file-related contexts should also offer `Add comment on file`; comment rows should offer reply, edit own comment, delete own comment, resolve, and reopen actions as applicable. Deleting comments must show the same invalidation warning flow as the web frontend before appending the delete event.
- `textDocument/diagnostic` or published diagnostics: show unresolved comments, stale anchors, failed mappings, and projected diagnostics from the real source buffer.
- `textDocument/documentSymbol`: expose a root review target label, file path symbols, readable hunk range symbols such as `lines 10-21` or `old lines 10-17`, and later thread/unresolved comment symbols for picker/outline workflows.

Peers should keep a row map for each synthetic review buffer:

```text
review row -> repo path, side, source line/range, source column mapping, thread/comment identity
```

Row metadata should be rich enough for context-aware actions: source rows include line/range anchor data, file and hunk rows include file scope, and comment rows include thread id, comment id, ownership/editability, and resolved state.

For current-side rows, Neovim should open the real file in a hidden source buffer so the user's normal Tree-sitter and language server setup attaches. Hidden buffers are used first for syntax mirroring, and Peers can then proxy LSP-like requests from the `peersdiff` buffer to that hidden real buffer. The bundled plugin may transparently wrap normal `vim.lsp.buf` entry points while the active buffer is a Peers review buffer so existing user mappings keep working, but source code actions must remain disabled so review-owned actions stay predictable. Deleted/base-side rows may initially provide Peers metadata only; hidden base-version buffers can be added later as a best-effort enhancement.

Because an LSP server cannot directly manipulate Neovim buffers, the Neovim integration will likely also need a small Neovim attachment layer. Keep that layer thin:

- Target Neovim 0.12 as the supported editor version for the bundled plugin.
- Lua provides the `:PeersReview` bootstrap and attaches the review buffer to the Peers session.
- The plugin should be installable from this repository as a normal Neovim runtime package with top-level `plugin/` and `lua/` files.
- The bootstrap should check the `pid` in `session.json`, discard stale session files when the process is gone, start a fresh Peers session, and stop or avoid stale `peersdiff` LSP clients so an old dead port is not reused after a session restart.
- If Neovim starts the Peers session, quitting Neovim should stop that child process by default. Sessions started outside Neovim must not be stopped by the plugin.
- If Peers needs to update Neovim buffers directly, the single Peers session process may connect to Neovim over Msgpack-RPC and use `nvim-rs`.
- Do not create a second long-lived Rust worker process for Neovim. The Peers session process should remain the single owner.

Realtime updates are required in Neovim too. Manual refresh is only a fallback when realtime is disabled or broken.

Neovim commands should stay small:

```vim
:PeersReview
:PeersSubmit
:PeersAskAgent
:PeersClose
:PeersRefresh
```

Most daily review actions should be available through normal LSP hover, definition, references, diagnostics, document symbols, and code actions.

## Frontend Layout

The review UI should feel close to GitHub's pull request review experience.

Desktop layout:

```text
top bar: review target, refresh, review tabs, unresolved count
left: file sidebar
center: active review tab content
```

The "Files changed" tab should be the primary/default tab. Comments and threads should be inline in the diff/full-file surface, similar to GitHub pull request review.

Use shadcn primitives:

- `Resizable` for panes
- `Sidebar` for file list
- `ScrollArea` for long lists and diffs
- `Popover` for inline comment composer
- `Textarea` for Markdown comments
- `Badge`, `Button`, `Tabs`, `ToggleGroup`, `Tooltip`, `Separator`

The first screen is the review workspace, not a landing page.

Compositional layout files should stay lean. For example, `ReviewWorkspace.tsx` should compose the toolbar, sidebar, diff viewer, inline thread layer, and quick access menu, but detailed row styling and primitive UI behavior should live in smaller component files.

## Review Navigation

Use top-level review tabs similar to GitHub pull requests:

- `Files changed`: primary/default tab. Shows the file sidebar and diff/full-file viewer.
- `Conversation`: shows all review comments and threads in one chronological scrollable page.
- `Commits`: shown only for branch review mode, not for `peers diff`, `peers diff --cached`, or `peers diff --all`.

The `Files changed` tab:

- Opens by default for every review.
- Keeps comments inline with the relevant diff or full-file lines.
- Keeps the file sidebar visible on desktop.

The `Conversation` tab:

- Provides a scrollable review-wide timeline of comments and threads.
- Groups thread activity clearly enough to understand the anchor, author, status, and latest replies.
- Links anchored threads back to their file and line/range in `Files changed`.
- Links file-level threads back to their file in `Files changed`.
- Includes resolved and unresolved threads, with status clearly shown.
- Allows creating review-level threads that are not attached to any diff, file, or line.
- Shows review-level threads in the same timeline as anchored threads, with a clear review-level label instead of a file/line anchor.
- Is an overview/history surface, not the primary commenting surface.

The `Commits` tab:

- Appears only when the review target is a branch/range review with meaningful commits between base and head.
- Lists commits in the reviewed range with enough metadata to identify them: abbreviated hash, title, author, and time.
- May be read-only in the first pass.

## File Sidebar

The left sidebar should always be available on desktop in the `Files changed` tab, including when viewing a full file.

Default:

- Show only changed files.
- Group files by parent directory path.
- Render one level of directory path groups and one level of file rows.
- Show file status badge.
- Show viewed state.
- Show unresolved comment count.
- Allow directory path groups to be collapsed and expanded.

Example:

```text
src/features/review/
  FileSidebar.tsx
  ReviewWorkspace.tsx

src/features/review/search/
  quickAccessSearch.ts
  reviewSearch.ts
```

Directory path group labels:

- Show the full parent path when there is enough space.
- Truncate from the beginning when the path is too long, preserving the most specific trailing path segments.
- Use the shadcn `Tooltip` primitive on hover to show the full path.
- Use a stable hit target for collapse/expand so long path labels do not shift the control.
- Root-level files should be grouped under a clear root label such as `/`.

File rows:

- Show only the basename as the primary label.
- Keep status, viewed state, and unresolved count on the file row.
- Use the full repo-relative file path for navigation, search, and tooltips when needed.
- Highlight the currently selected/viewed file, whether the center pane is showing a diff or full-file view.
- Keep the selected file visible in the sidebar when practical, expanding its parent path group if needed.

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
- Near the file header for file-level comments.
- In the `Conversation` timeline.
- As counts in the file sidebar.

Comments use plain Markdown text in a textarea.

No rich text editor initially.

Comment presentation:

- Comment cards should be dense but complete; avoid sparse cards that only show body text.
- Each comment shows author display name and created date/time.
- Prefer a compact relative time in the main card, with exact timestamp available on hover or in a tooltip.
- Do not show raw author kind labels such as `human` or `agent` in the card body.
- Agent comments should be visually distinguishable with a small icon or badge; an icon is enough when the author name already makes the source clear.
- Edited comments should show an edited marker with enough timestamp detail to understand when the edit happened.
- Thread status such as resolved/outdated/detached should be shown at the thread level, not repeated on every comment.

Editing and deletion:

- Users can edit their own comments.
- Users can delete their own comments.
- Users can delete a whole thread when they are allowed to delete the thread's root comment.
- Neovim must follow the same edit/delete rules as the web UI. Delete comment should be available from comment-row code actions for comments the current author can edit/delete, and must show the same warning before invalidating later dependent activity.
- Editing or deleting a user comment invalidates later dependent activity in that thread.
- Dependent activity includes following agent comments, following agent-created replies, and later resolved/reopened status changes that happened after the edited/deleted comment.
- Before applying an edit/delete that would invalidate later activity, show a confirmation warning that those later comments/status changes will be removed from the visible thread state.
- After confirmation, the UI should remove the invalidated later activity from the visible thread and from generated review summaries/agent context.
- Because storage is append-only, do not physically rewrite old JSONL lines. Record edit/delete/invalidation events and let derived state hide the invalidated activity.

Inline thread behavior:

- Existing threads render directly below their anchored line or range in both diff and full-file views.
- Multiple threads on the same line or range render in a stable order by creation time.
- Resolved threads may be collapsed by default, but unresolved threads should be visible without opening a side panel.
- Reply, edit, delete, resolve, and reopen actions are available from the inline thread.
- Creating a new thread opens the composer inline at the selected range after the comment affordance is clicked.
- If an anchor becomes outdated or detached, show the thread inline at the best relocated position when possible; otherwise show it in a clear detached/outdated section for that file.

File-level thread behavior:

- File-level threads are created from a file header action labeled `Comment on this file`.
- They are attached to the file path, not a diff hunk or line range.
- They render near the file header in the `Files changed` tab and at the top of full-file view.
- They support the same reply, edit, delete, resolve, and reopen operations as line/range threads.
- They should appear in file sidebar unresolved counts for that file.
- They should be included in global unresolved counts and quick access comment search.

Review-level thread behavior:

- Review-level threads are created from the `Conversation` tab.
- They are not attached to a file, diff hunk, or line range.
- They support the same reply, edit, delete, resolve, and reopen operations as anchored threads.
- They should not appear in file sidebar unresolved counts.
- They should be included in global unresolved counts and quick access comment search.

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

Comment search should search all comments in the current review, regardless of the file visibility filter. Selecting an anchored or file-level comment in a currently hidden unchanged file should open the file directly and indicate that it is outside the current file filter. Selecting a review-level comment should open it in the `Conversation` tab.

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
      path?: string
      lineLabel?: string
      authorName: string
      excerpt: string
      resolved: boolean
      scope: "anchored" | "file" | "review"
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

## Feature Status

Use this section as the short source of truth for implementation progress. Update it whenever a feature moves materially closer to or farther from the spec.

Statuses:

- `Complete`: implemented and believed to follow the current spec.
- `Partial`: implemented enough to use or preview, but known gaps remain.
- `Planned`: specified, but not meaningfully implemented.
- `Out of date`: implemented behavior exists but conflicts with the current spec.

Current status:

| Feature | Status | Notes |
| --- | --- | --- |
| Project rename to Peers | Complete | CLI/package/docs use `peers`, `.peers`, and `PEERS_*`. |
| CLI skeleton | Partial | Commands exist and review UI launch starts the local Vox server; browser auto-open and packaging polish remain. |
| Review storage/event log | Partial | Append-only JSONL storage exists for review/comment/file-viewed/submit flows. |
| Author detection and overrides | Complete | Git config, CLI flags, `PEERS_*` env vars, and agent fallback identity are implemented. |
| CLI comment operations | Partial | Add/reply/edit/delete/resolve/reopen plumbing exists for line comments; file/review-level CLI creation is not implemented. |
| Generated review and agent context files | Partial | Basic generated files exist and replay hides invalidated dependent activity; richer file/review-level coverage still needs polish. |
| Git diff loading | Partial | Working tree, cached, all-changes, and branch targets load real Git diffs into the compact payload. This currently shells out to `git` for diff data rather than using `gix` end-to-end. |
| Arborium highlighting | Planned | Not implemented. |
| Vox RPC service | Partial | Local WebSocket service exposes review load, refresh, comment mutations, viewed files, and submit review; dev UI consumes generated TypeScript client. |
| Realtime UI updates | Planned | Specified for event-log, agent/CLI comment, UI comment, viewed/submitted, and diff/file-change updates; not implemented. |
| Neovim review mode | Partial | The local Peers session starts a `peersdiff` LSP endpoint using `tower-lsp-server`, `peers nvim` can launch the current review session, Vox/LSP share a cloneable review provider, and Lua `:PeersReview` opens a full-focus synthetic review buffer. Rust serves `peers/renderReview` with rendered diff rows, row metadata, structural highlights, and document symbols; Lua applies rows, mirrors viewport-scoped Tree-sitter highlights from hidden current-side source buffers, opens a floating writable composer for add-comment code actions, and proxies hover/definition/declaration/type-definition/implementation/references from mapped current-side rows into hidden source buffer LSP clients. Line comments render inline at their anchor row, and code actions are becoming context-aware for line/range/file/comment rows. Realtime buffer updates, diagnostics, reply/edit/delete/resolve/reopen mutations, and delete invalidation confirmation are still missing. |
| Review workspace layout | Partial | Toolbar, sidebar, diff surface, full-file route, quick access, sticky diff headers, and empty-diff state exist. Conversation/Commits tabs are still missing. |
| Frontend review payload shape | Complete | Frontend consumes server-provided files, per-path file content, per-path compact diffs, and thread data through TanStack Query. |
| Inline comments in diff/full-file views | Partial | `git-diff-view` renders real diffs with inline composers, persisted threads, multi-line selection, and range rails. Full-file comments still rely on the current full-file route behavior rather than the same library surface. |
| File-level comments | Partial | `Comment on this file` creates persisted file-level threads; broader Conversation/quick-access treatment still needs completion. |
| Review-level comments | Planned | Specified for the `Conversation` tab; not implemented. |
| Conversation tab | Planned | Specified as all-comments timeline; not implemented. |
| Commits tab | Planned | Specified for branch/range reviews; not implemented. |
| File sidebar path grouping/collapse | Complete | Sidebar groups by parent path, supports collapse/expand, shows basename rows with status/viewed/comment counts, preserves active-file highlighting, and uses tooltips for full paths. |
| Full-file persistent sidebar | Partial | Full-file route keeps the sidebar visible and can show full file content; current-file highlighting and full-file parity still need work. |
| Unchanged-file toggle | Partial | Toggle and routing exist; behavior still needs verification against all routes. |
| Quick access menu | Partial | File/comment search exists against live review data; review/file/review-level scope navigation is not complete. |
| Comment card presentation | Partial | Inline cards, edit/delete warnings, resolve/reopen, and agent identity display exist; finer timestamp/icon polish remains. |
| Packaging embedded frontend assets | Planned | Not implemented. |

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
16. Add realtime update notifications for event-log and diff changes.
17. Add Neovim review mode with a `peersdiff` LSP and single Peers session attachment.
18. Add packaging path for embedded frontend assets.
