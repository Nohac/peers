# Peers Spec

Peers is a local Git review tool. It stores durable repo-scoped review comments in the project, then projects those comments into live diff, branch-review, and editor views so humans and AI agents can read, create, and respond to review feedback without manually managing review sessions.

Slogan:

```text
Local Git peer review for humans and agents.
```

## Goals

- Review unstaged, staged, full working tree, and branch-range diffs locally.
- Select one or more diff/file lines and create comment threads.
- Let humans and AI agents add, edit, delete, reply to, resolve, and reopen comments.
- Keep comments independent of a specific review session. Comments should follow code across staged/unstaged views, branch creation, branch switches, commits, and branch reviews when anchor relocation still deems them relevant.
- Make Neovim the primary product surface while the review model is stabilizing.
- Store review data in a local project folder with an append-friendly format.
- Use Rust, gitoxide for Git access, `peersdiff` LSP for Neovim RPC, facet for serialization, and Neovim as the primary UI surface.
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
- `facet` and `facet-json` for serialization
- `thiserror` for custom domain errors
- Arborium for server-side syntax highlighting

Web frontend:

- Removed while the repo-scoped provider and Neovim-first workflow stabilize.
- Recover from Git history later if a web surface becomes useful again.

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
```

Do not organize by generic category names like `types.rs`. Put data structures next to the behavior that owns them.

Suggested backend ownership:

- `cli.rs`: command parsing and command dispatch.
- `diff.rs`: review target resolution, gitoxide diff loading, diff normalization, highlighting integration.
- `review.rs`: view/projection construction, generated review artifacts, and compatibility code while older review-scoped storage is being removed.
- `comments.rs`: repo-scoped event model, JSONL parsing/encoding, replay, comment commands, anchor relocation inputs, and agent context rendering.
- `review_provider.rs`: cloneable async review provider used by Neovim LSP and local CLI commands.
- `server.rs`: local session process, realtime watcher, and Neovim LSP startup.

## CLI

Use Clap derive for CLI parsing.

Primary commands:

```bash
peers thread list
peers thread list --all
```

Session startup commands are internal/hidden and primarily launched by the Neovim plugin:

```bash
peers session diff
peers session diff --cached
peers session diff --all
peers session review --base main --head HEAD
```

Thread commands:

```bash
peers thread add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body "This bypasses validation."

peers thread add \
  --path src/foo.rs \
  --side new \
  --lines 42:47 \
  --body-file -

peers thread reply thr_123 --body "I fixed this."
peers thread reply thr_123 --body "I fixed this." --resolve
peers thread reply thr_123 --body-file -
peers thread list
peers thread list --status open
peers thread list --status open --context
peers thread list --status open --context 5
peers thread list --status complete
peers thread list --scope repo
peers thread list --all
peers thread show thr_123 --context 8
peers thread show thr_123 --context 8 --evidence
peers thread show thr_123 --context 8 --no-evidence
peers thread edit cmt_123 --body "Updated comment."
peers thread delete cmt_123
peers thread accept cmt_agent_123
peers thread decline cmt_agent_123 --body "This is intentional because ..."
peers thread resolve thr_123
peers thread reopen thr_123
```

`peers thread list` follows current visibility rules by default. Use `--all` to include hidden or no-longer-relevant threads such as resolved threads from older commits.

Cleanup commands:

```bash
peers clean
peers clean --dry-run
peers clean --status complete
peers clean --older-than 30d
peers clean --detached
peers clean --hidden
peers clean --no-interactive
```

`peers clean` must be conservative by default. It should not mutate state without an interactive confirmation unless `--no-interactive` or an equivalent explicit non-interactive flag is passed. Default cleanup should only target safe state, such as complete/resolved threads that no longer show in the current projection and are older than a grace period. Unresolved threads must not be deleted by default.

Agent support:

```bash
peers skill
peers thread list
peers thread --agent "Codex (GPT-5)" reply ...
peers agent codex
peers agent attach --addr ws://127.0.0.1:4500
peers agent -- codex --remote %addr
```

`peers agent` is a launcher/session wrapper, not a model API client. It requires either an explicit supported agent subcommand such as `peers agent codex` or an explicit passthrough command after `--`, such as `peers agent -- codex --remote %addr`; running `peers agent` without one of those forms prints the agent help. It should start or attach to a local agent server, write `.peers/agent-session.json`, expand template variables for the selected preset or passthrough command, and then run the external agent command. Peers owns the local session metadata and prompt construction; the external agent owns authentication, model selection, tools, permissions, and its normal TUI behavior.

Template variables may be used by future configurable presets:

```text
%addr       Full agent server address, such as ws://127.0.0.1:4500
%host       Agent server host, usually 127.0.0.1
%port       Agent server port
%repo       Repository root
%session    Path to .peers/agent-session.json
```

The built-in Codex convenience preset:

```bash
peers agent codex
```

expands to:

```bash
codex --remote %addr
```

The explicit passthrough form remains supported for custom local agent commands:

```bash
peers agent -- <command> [args...]
```

Use a loopback websocket listener for local Codex integration for now. Peers should allocate a free local port by binding `127.0.0.1:0`, start `codex app-server --listen ws://127.0.0.1:<port>`, validate the endpoint, and then launch the user's agent command with `%addr` expanded to the selected address. This avoids hardcoding ports while also avoiding Codex's implicit Unix socket path behavior, where `unix://` does not reveal a stable path and explicit paths may fail depending on the environment. If the port is claimed between allocation and app-server startup, Peers should retry with a newly allocated port.

Existing agent sessions can be attached explicitly:

```bash
peers agent attach --addr ws://127.0.0.1:4500
```

Attach should validate that the local app-server endpoint is reachable before writing `.peers/agent-session.json`. Neovim agent actions should read `.peers/agent-session.json`; if no usable agent session exists, they should show a concise prompt suggesting `peers agent codex` or `peers agent attach --addr <addr>`.

The Neovim integration should route agent engagement through the existing `peersdiff` LSP connection rather than shelling out from Lua. The first narrow surface is a prompt-based `peers/askAgent` custom method exposed as `:Peers agent <prompt>`. The backend reads `.peers/agent-session.json`, connects to the Codex app-server websocket, finds the most recent loaded thread for the repo working directory, and submits a `turn/start` text input. Codex-specific protocol and websocket details belong in a dedicated Rust module; the higher-level agent/session module should remain responsible for launcher/session metadata. A later refactor should introduce a small generic agent trait before adding non-Codex providers.

Future `.peers/config.toml` support should allow local presets without hardcoding one agent:

```toml
[agent]
default = "codex"

[agent.presets.codex]
listen = "ws"
command = ["codex", "--remote", "%addr"]
```

The first implemented agent engagement actions should be:

- Review open comments without code changes.
- Create a Peers comment and immediately ask the agent to respond/follow up.
- Ask the agent to fix selected/open comments, with instructions to reply and resolve through `peers thread reply --resolve`.

Any Peers command that needs live repo state should attach to the local Peers process when one is running, or start one when necessary. Commands that only print static help, such as `peers skill`, do not need a session. Neovim is an attachment surface for the same process rather than a separate command namespace.

An active session should publish ephemeral connection information at:

```text
.peers/session.json
```

The file includes the process id, current repo/view metadata, `peersdiff` LSP URL, realtime flag, Neovim listen address, and start time. The session file is an attachment hint for local clients, not canonical comment state.

Environment overrides:

```bash
PEERS_AGENT="Codex (GPT-5)"
PEERS_AUTHOR_NAME="Jonas"
PEERS_AUTHOR_EMAIL="jonas@example.com"
```

## Review Modes

`peers session diff`:

- Reviews unstaged changes.
- Equivalent intent to `git diff`.

`peers session diff --cached`:

- Reviews staged changes.
- Equivalent intent to `git diff --cached`.

`peers session diff --all`:

- Reviews all current changes from `HEAD` to working tree, including staged and unstaged changes.

`peers session review`:

- Reviews the current branch against `main` by default.
- Uses merge-base of base branch and `HEAD`.

`peers session review --base <rev>`:

- Reviews merge-base of `<rev>` and `HEAD` to `HEAD`.

Explicit range:

- Future-compatible with `--base <rev> --head <rev>`.

## Runtime

When opening the UI:

- Discover the Git repo root.
- Start or attach to a repo session and compute the requested view projection.
- Start a localhost server on `127.0.0.1:<random-port>`.
- Generate a one-use token.
- Open or attach the Neovim review buffer when launched from the editor.
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

Agent identity must be explicit for agent-authored comments and agent-launched Peers sessions. `peers thread --agent <identity>` records the provided identity directly, and root `peers --agent <identity>` can be used for agent-launched sessions. There is no generic fallback agent name.

Identity is descriptive, not a security boundary. Edit/delete ownership is soft and local.

## Storage

Canonical Peers state is stored inside the reviewed project and is scoped to the repository, not to individual review sessions:

```text
.peers/
  events.jsonl
  threads/
    thr_01j/
      thread.json
      comments/
        cmt_01j.json
  session.json
  review.md
```

Canonical state is split between a lightweight append-only action log and per-thread payload files.

`events.jsonl` is a lightweight repo-scoped log of things that happened. It should contain event kind, ids, timestamps, actor, and small transition metadata. It should avoid carrying large or repeatedly edited payloads such as full comment bodies when those can live in per-thread files.

`threads/<thread-id>/thread.json` stores the thread payload: current anchor evidence, creation provenance, status, and small thread metadata.

`threads/<thread-id>/comments/<comment-id>.json` stores comment payload: author, body, created/edited/deleted metadata, and any local revision metadata needed to explain edits.

Diff and review modes are live projections over this repo-scoped state, not owners of comment state.

`review.md` is generated for humans. Agents should inspect threads through `peers thread list --context` and `peers thread show <thread-id> --context`.

File-level and repo/review-level comments should also be included in generated review summaries and CLI thread context output.

Use append-only JSONL events for the action log so agents can append safely and merge conflicts stay manageable. Payload files should stay small and individually addressable so comment bodies are easy to inspect and cleanup/compaction does not require rewriting one large log.

Creation mode is provenance only. A thread created from cached-diff review mode may still appear in working-tree review mode, a branch review, another branch, or Neovim if the anchor relocation algorithm considers it relevant. The original mode must not be used as a hard visibility filter.

Example events:

```json
{"kind":"thread_created","thread_id":"thr_01j","comment_id":"cmt_01j","created_at":"2026-05-28T12:15:30Z","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"}}
{"kind":"comment_added","thread_id":"thr_01j","comment_id":"cmt_02j","created_at":"2026-05-28T12:18:02Z","author":{"kind":"agent","display_name":"Codex","email":null}}
{"kind":"agent_comment_accepted","thread_id":"thr_01j","comment_id":"cmt_02j","accepted_at":"2026-05-28T12:20:00Z","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"}}
{"kind":"thread_resolved","thread_id":"thr_01j","resolved_at":"2026-05-28T12:22:10Z","author":{"kind":"human","display_name":"Jonas","email":"jonas@example.com"}}
```

Event kinds:

- `review_metadata_updated`
- `thread_created`
- `comment_added`
- `comment_edited`
- `comment_deleted`
- `agent_comment_accepted`
- `agent_comment_declined`
- `thread_title_updated`
- `thread_resolved`
- `thread_reopened`
- `thread_collapse_updated`
- `thread_anchored`
- `thread_archived`
- `thread_pruned`

Derived state is rebuilt by loading thread/comment payloads and replaying events.

Agent comment disposition is distinct from thread resolution. An agent-authored comment may be pending, accepted, or declined. Accepting an agent comment means a human has acknowledged the feedback as valid or useful; it does not automatically apply code and does not automatically resolve the thread. Declining an agent comment means a human has explicitly rejected that feedback; if the declined comment is the only remaining actionable item in the thread, the UI may offer `Decline and resolve`, but the storage model should still record the decline and the resolve as separate events. The latest disposition event for a comment wins.

Threads may have an optional short title stored in the thread payload and updated through an append-only `thread_title_updated` event. The title is display metadata for collapsed/resolved thread rows and summaries; it should be concise enough to remind the reader what the thread was about without reopening it. When an agent resolves a thread, it must set or update this title as part of the resolution flow. Human resolution may allow an optional title, but should not require one.

Threads may also store durable UI metadata such as `collapsed: bool` in the thread payload. Toggling collapsed state should update the payload and append a lightweight `thread_collapse_updated` event containing the thread id, timestamp, author, and new collapsed value.

`thread_archived` and `thread_pruned` are intended for `peers clean`. Prefer append-only cleanup events first. A later explicit compaction command may rewrite the event log into a smaller canonical form, but ordinary cleanup should not silently rewrite history.

## Views, Anchors, and Visibility

Peers comments are durable. Diff/review/editor surfaces are projections.

Every visible mode should follow the same pipeline:

1. Load repo-scoped thread/comment payloads and the action log from `.peers/`.
2. Load the current Git/file snapshot for the requested view: working tree, cached, all changes, or branch range.
3. Relocate each thread anchor into that snapshot.
4. Assign a placement: inline, file-level, repo/conversation-level, detached, or hidden by policy.
5. Render the projection in CLI, Neovim, or any future UI.

Line/range anchors should store layered evidence:

- original path and old path when known
- side and original line/range
- selected text
- selected/range hash
- per-line hashes
- nearby context text and context hashes, including original before/after snippets captured at creation time
- creation provenance such as branch name, head oid, merge-base oid, and view kind

Anchor relocation should try, in order:

1. Same path or renamed path with exact selected/range hash.
2. Same path with exact per-line hashes for the selected range.
3. Same path with before/after context match.
4. Any changed file with exact selected/range hash.
5. Any tracked file with exact selected/range hash, to catch moved or split code.
6. Same file with approximate text similarity, only when confidence is high enough to avoid silently attaching feedback to unrelated code.
7. Original line/range numbers as a weak fallback.
8. File-level fallback.
9. Detached.

Multi-line anchors need a bounded expansion pass before weak line fallback. If most original per-line hashes still appear in order inside a slightly larger current window, such as when a line is inserted inside a commented function, the relocated block should expand to cover that current window instead of marking the whole thread stale. This placement should carry per-line mapping/confidence metadata: unchanged original lines stay strong, changed original lines are marked as changed/context, inserted current lines inside the relocated block are marked as `gap`, and deleted original lines inside the relocated block are marked as `missing`. `gap` and `missing` are not failure states by themselves; they mean the surrounding block is still attached but has internal insertions or deletions. Internal edit runs are allowed when bracketed by strong content before and after. Inserted-only edge gaps and trailing missing selected lines may still be accepted when overall exact evidence is strong; leading missing or changed edge source lines remain suspicious. Staleness should be based on evidence quality and drift rather than fixed counts: a window with at least 50% exact line evidence can move farther, while a window below 50% exact evidence should only remain attached if the relocated start/end stay close to the original range. Candidate windows should be generated by indexing current file lines by raw hash, mapping original evidence lines into those hash buckets, clustering the closest ordered matches, and deriving candidate ranges from those proximity clusters rather than scanning every possible expanded window. Stored anchors may keep hash strings for JSON, but relocation should convert them at the boundary and compare raw hash values internally. Line hashes are evidence, not identity: low-information lines such as blanks, braces, punctuation, common derive attributes, or hashes with many occurrences should not create relocation candidates by themselves, though they may still be mapped once a block is bracketed by stronger evidence. The expansion must stay conservative, prefer the smallest confident window, avoid splitting one original block across unrelated regions, and have table-driven tests for single-line, multi-line, inserted-line, changed-line, missing-line, multi-gap, edge-gap-with-strong-evidence, trailing-missing-with-strong-evidence, low-similarity-near-origin, low-similarity-far-origin, moved-window, repeated-low-signal, and ambiguous-window cases.

Line-number fallback must not be treated as a trusted relocation. If content and context evidence no longer match but the original line/range still exists, the placement should be marked as weak/stale/line-fallback in projection metadata. Unresolved threads may still render at that weak placement so the user can act on them, but resolved threads with weak placement should hide from default diff/editor projections and remain available only in explicit complete/global listings and cleanup previews.

Projection relocation runs over an immutable current diff snapshot and anchor indexes. The provider builds the anchor indexes once, shares them with `Arc`, spawns Tokio tasks for visible thread relocation, and awaits the join handles together while preserving projection order. Do not spawn ad hoc OS threads inside provider code. If very large reviews make task counts a problem, add bounded Tokio concurrency rather than changing the relocation semantics.

In visual review surfaces, stale/weak placements should be immediately distinguishable from confident inline placements. The normal thread rail/card border can stay blue for exact or strong relocations, while weak/stale/line-fallback placements should render that same border/rail in red. This should be metadata-driven from the placement state rather than inferred by the UI from text labels.

Unresolved comments should be surfaced aggressively. If an unresolved anchor can only be weakly relocated, show it with moved/stale/changed metadata rather than hiding it. If it cannot be relocated, keep it visible in an unresolved detached section and diagnostics/list output.

Open comments are part of the projection, not decoration on top of changed hunks. If the current Git diff has no changed files or no hunk for a commented region, unresolved comments should still create enough review surface to read and act on them. For line/range comments, render a synthetic comment-context hunk around the relocated anchor with unchanged source lines and the inline thread. For file-level comments, render the file header and thread even if the file is currently unchanged. For detached unresolved comments, render a detached section instead of hiding them behind an empty state.

Resolved comments should be hidden more aggressively as context changes. Exact matches may remain available behind a show-resolved option. Weak matches, file-only matches, and detached resolved comments should be hidden from normal diff/editor projections by default, while remaining available through explicit complete/resolved/global listing and cleanup previews.

`peers thread list --context [lines]` should eventually render context from the projection, not only from live file line numbers. When an anchor still relocates cleanly, it can show current source context. When the source has drifted, moved, or detached, it should fall back to the stored original selected text and before/after snippets so agents can still see what the comment originally referred to. The output should label current, moved/stale, and original-only context distinctly.

`peers thread show <thread-id> --context [lines]` should include the current anchor placement status. It should print stored original evidence automatically when the current placement is not exact, support `--evidence` to force original evidence, and support `--no-evidence` to suppress it. Original evidence should include the stored path, side, line/range, creation provenance, selected text, and before/after context. Stored hashes remain relocation internals and should not be shown in normal human/agent output.

Peers should support explicitly updating a thread or comment anchor from the current projected context. This is useful when a human or agent has verified that a relocated/stale placement is the intended new source location and wants future relocation to use the updated text, range hashes, per-line hashes, before/after context, path, and line/range as the new evidence. The operation should be explicit, such as `peers thread update-context <thread-id>` or a Neovim code action labeled `Update thread context from current location`, and should append an event rather than silently rewriting history. The old anchor/evidence should remain available in history for audit/debugging. Updating context should not resolve the thread, accept an agent comment, or change comment body text; it only refreshes attachment evidence.

## Cleanup

`peers clean` is the explicit state cleanup mechanism.

The default behavior should be conservative:

- require an interactive confirmation before mutating state
- do nothing in non-interactive contexts unless `--no-interactive` or an equivalent explicit flag is passed
- archive/prune only safe candidates, such as complete/resolved threads that are hidden from the current projection and older than a grace period
- never archive/prune unresolved threads by default

Useful cleanup rules:

- `safe`: resolved, hidden from the current projection, and older than the configured grace period
- `detached`: anchor cannot be relocated
- `hidden`: not visible under the current default projection policy
- `old`: created or resolved before a requested age threshold
- `branch-gone`: created from a branch that no longer exists, only when resolved

`peers clean --dry-run` should print the candidate threads and the reason each one would be cleaned. `peers clean` should show the same summary before prompting. Cleanup should initially append archive/prune events rather than rewriting the action log or payload files.

## IO Boundary Rule

Keep filesystem/path code as a thin shell. Any meaningful behavior should operate on loaded data, buffers, readers, writers, cursors, or strings.

Use Tokio filesystem APIs for real IO.

Preferred:

```rust
async fn parse_events(input: &str) -> Result<Vec<PeersEvent>>;
async fn parse_events_from_reader(reader: impl AsyncBufRead + Unpin) -> Result<Vec<PeersEvent>>;
fn encode_event(event: &PeersEvent) -> Result<String>;
fn replay_events(events: &[PeersEvent], payloads: &PayloadStore) -> Result<PeersState>;
async fn render_review_markdown(state: &PeersState, target: Option<&ReviewTarget>, out: impl AsyncWrite + Unpin) -> Result<()>;
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

Projection payload shape:

- `files`: ordered list of `ReviewableFile` metadata for the sidebar and file headers.
- `fileContentsByPath`: map keyed by repo path. Each value contains `old` and/or `new` line arrays for full-file rendering.
- `fileDiffsByPath`: map keyed by repo path. Each `FileDiff` contains hunks with optional old/new line ranges and ordered compact sections.
- `threads`: ordered list of `CommentThread` records anchored by path, side, and line/range, with file-level and review-level scopes added later.

Diff hunks must point back into `fileContentsByPath`:

- Ranges are 1-based and inclusive.
- Context sections contain old and new ranges.
- Added sections contain a new range.
- Removed sections contain an old range.
- Renderers expand sections into rows and read text from `fileContentsByPath`.
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
- selected/commented states come from renderer styling

## RPC

The active local RPC surface is the `peersdiff` LSP endpoint. Neovim uses standard LSP requests for source-buffer code actions and Peers custom methods for review rendering, comment mutations, thread collapse state, and agent engagement. If a future non-Neovim local client needs a dedicated API, add a new contract around the same `ReviewProvider`.

## Realtime Updates

Open Peers projections should stay current without manual refresh whenever local comment state or relevant Git state changes.

Sources that must update the open UI:

- Local file changes that alter the reviewed diff.
- CLI comment operations, including human and agent `peers thread add/reply/edit/delete/resolve/reopen` commands.
- Comment operations from another open Neovim instance or future UI.
- File viewed/unviewed changes.
- Review submission events.
- Regenerated `review.md` output when its source event log changes.

Implementation expectations:

- Treat `.peers/events.jsonl` plus `.peers/threads/` payload files as the canonical repo comment source.
- Watch the repo event log for append changes and notify connected clients.
- Watch relevant Git working tree/index inputs for diff changes, respecting repository `.gitignore` rules, and notify connected clients that the diff payload should refresh.
- The Neovim review buffer should refresh through the active `peersdiff` LSP session when it receives a comment or diff update notification.
- Update notifications may be coarse-grained at first, such as `review_changed` and `diff_changed`; they do not need per-entity patches in the first pass.
- UIs should keep local interaction state where practical, such as active file, active comment, cursor/scroll location, and composer draft, while replacing server-owned review data.
- If an update invalidates the currently visible file, comment, or line anchor, the UI should fall back gracefully to the nearest valid review surface instead of crashing.
- Avoid polling as the primary strategy. A small debounce around file watcher bursts is acceptable.

## Neovim Integration

Peers should provide a first-class Neovim review mode for keyboard-driven code review without requiring the user to leave their editor. Neovim is the primary product surface while the repo-scoped comment architecture stabilizes.

The integration should keep one Peers session process per repository/view attachment. The session process owns repo-scoped comment state, Git watchers, event log watchers, and Neovim-facing services. Launching from either side should attach to the same active repo session:

- hidden `peers session diff` and `peers session review` commands start a repo session and publish local connection information in `.peers/session.json`.
- `:Peers` in Neovim attaches to an existing session for the current repo when one exists.
- If no session exists, Neovim may start the same Peers process that the CLI would start, then attach to it.
- Neovim and any future UI must see the same repo-scoped event log, realtime updates, and generated artifacts.

The core operations should live behind one cloneable async provider that can render live projections from repo-scoped comments and Git state. The `peersdiff` LSP should call the same provider directly inside the Peers session process. The provider should avoid external shared mutability by default; if live state becomes necessary, prefer an internal event loop, request/response channels, or purpose-built concurrent maps over `Arc<Mutex<_>>`.

The Neovim surface should be one full-focus synthetic review buffer, not a split-based UI:

```vim
buftype=nofile
bufhidden=hide
buflisted=true
swapfile=false
modifiable=false
readonly=true
filetype=peersdiff
```

Expected behavior:

- The review buffer should appear as a normal listed buffer so users can find it through familiar buffer pickers, including Telescope.
- The review buffer should be easy to reopen with `:Peers` and should remain usable through the jumplist where practical.
- The buffer should render files, changed hunks, comment-context hunks, added/removed/context lines, inline threads, multiline range markers, file-level comments, review-level conversation entries, and compact unchanged-context placeholders.
- File context labels, including file header rows and editor breadcrumb/document-symbol context, should include compact Git-style status markers such as `[A]`, `[D]`, `[R]`, `[M]`, `[U]`, or `[B]` for added, deleted, renamed, modified, unchanged, and binary files.
- When there are no changed files but there are unresolved or otherwise visible comments, the Neovim review buffer should render the relevant commented files/regions as comment-context hunks rather than showing an empty state.
- The `No file changes` empty state should only render when the current projection has no changed files and no visible unresolved/relevant comments. In normal default views, that usually means all comments are resolved or hidden by policy and the Git diff is empty.
- Peers owns the render model. The `peersdiff` LSP exposes a custom `peers/renderReview` request that returns synthetic review-buffer lines, sidebar panel lines, row metadata, structural highlights/extmarks, and symbol metadata derived from the shared review provider.
- The Lua attachment layer applies rendered lines and extmarks to the synthetic review and sidebar buffers, while Rust remains the source of truth for row semantics, review data, sidebar formatting, truncation, and highlight spans.
- Structural highlights cover file headers, hunk headers, line numbers, add/delete gutter markers, and comment rows. Prefer gutter/prefix color over full-line color unless a stronger visual treatment is required.
- Inline comment threads and the comment sidebar should share the same box-drawing shape: `╭─ <status glyph> <label>`, `│ <author/time>`, `│ <body>`, and `╰─ <anchor status>`. Inline diff threads include the repo path in the label and collapse expanded threads only when they exceed six messages. Sidebar comment threads omit the path from the label and collapse thread bodies when they exceed three messages. Expanded inline headers should use the compact open/resolved glyph instead of `[Open]`/`[Resolved]`, should not show `n comment(s)`, and should rely on the footer for anchor status such as `stale line fallback`.
- Current-side added and context rows should mirror syntax/highlight state from hidden real file buffers so the review buffer follows the user's Neovim theme and language setup. This mirroring must be viewport-scoped for performance on large reviews. The current group-based approach can use Neovim inspection APIs such as `vim.inspect_pos`; if group/link behavior keeps diverging from visible source buffers, a future improvement is to resolve the inspected highlight stack into concrete foreground/style attributes per range, cache Peers-owned highlight groups by those attributes, and apply those concrete groups in the review buffer. Source background colors should usually not be copied because diff rows have Peers-owned backgrounds. Deleted/base-side rows may start with structural diff coloring only.
- If Neovim has an unsaved modified buffer for a reviewed file, the synthetic review buffer must not render that file's diff. It should show a clear per-file warning instead and publish an error diagnostic on the review buffer so normal diagnostic navigation can find it. Peers should never silently reload or overwrite a modified user buffer.
- Do not depend on Neovim folds or persistent split sidebars for the primary workflow.
- The review buffer remains read-only. Comment bodies are entered through a focused writable composer, such as a temporary floating buffer.
- Neovim comment composers should use native editor primitives. The default composer is a writable floating scratch buffer anchored near the relevant review row; `<Enter>` should remain available for newlines, and submission should use an explicit mapping such as `<C-s>`.

Peers should expose a `peersdiff` language server for this synthetic buffer. Use the `tower-lsp-server` crate from `tower-lsp-community/tower-lsp-server`, the maintained community fork of `tower-lsp`, unless a better maintained LSP server crate is chosen before implementation.

The LSP custom request boundary currently uses `ls_types::LSPAny`, which is effectively a `serde_json::Value`-shaped payload. Peers should still avoid `serde`, `Serialize`, and `Deserialize` on project data types; Rust render/mutation DTOs should be `Facet` types and converted at the LSP boundary through `facet-json` or a small direct `Facet` to `LSPAny` bridge. Hand-built `LSPAny` objects are acceptable as a temporary boundary detail, but they should not become the long-term render model.

The `peersdiff` LSP should let users keep their existing LSP mappings instead of defining Peers-specific replacements:

- `textDocument/hover`: show Peers metadata for review rows and proxy source hover for mapped current-side code rows.
- `textDocument/definition`: proxy from mapped current-side code rows to the hidden real source buffer's attached language servers and open the real target in the current window.
- `textDocument/references`: proxy references from mapped current-side code rows where practical, using the user's normal quickfix/location-list behavior.
- `textDocument/codeAction`: expose context-aware review actions only. Source LSP code actions from hidden buffers should not be proxied into the review buffer. Source rows should offer labels such as `Add line comment` or `Add comment on lines 3..9`; file-related contexts should also offer `Add comment on file`; comment rows should offer reply, edit own comment, delete own comment, resolve, and reopen actions as applicable. Agent-authored comment rows should also offer `Accept agent comment` and `Decline agent comment` when the current author is not that agent and the latest disposition is still pending. Deleting comments must show the invalidation warning flow before appending the delete event.
- `textDocument/diagnostic` or published diagnostics: show unresolved comments, stale anchors, failed mappings, and projected diagnostics from the real source buffer.
- `textDocument/documentSymbol`: expose a root review target label, file path symbols, readable hunk range symbols such as `lines 10-21` or `old lines 10-17`, and later thread/unresolved comment symbols for picker/outline workflows.

Peers should keep a row map for each synthetic review buffer:

```text
review row -> repo path, side, source line/range, source column mapping, thread/comment identity
```

Row metadata should be rich enough for context-aware actions: source rows include line/range anchor data, file and hunk rows include file scope, and comment rows include thread id, comment id, comment body, author kind, ownership/editability, invalidation risk, resolved state, collapsed state, and any agent comment disposition.

Neovim sidebar:

- The Neovim integration should provide an optional narrow right sidebar for review navigation.
- The sidebar must use a dedicated read-only `nofile` buffer in a fixed window with `winfixbuf` and `winfixwidth` enabled, so moving into the sidebar cannot replace the sidebar buffer. Buffer-switching commands inside the sidebar are intentionally blocked by Neovim; use `p` or `q` before normal buffer/window operations.
- Sidebar visibility is driven by the active window: while the sidebar is focused, live updates and resizes must keep it open and preserve focus unless the user presses `q` or `d`; while the main review/diff window is focused, hide the sidebar when that window is 90 columns or narrower, and keep or restore it above 90 columns unless the user explicitly closed it with `q`.
- The sidebar has at least two modes: changed/visible files and comment/thread overview. It should preserve its mode and cursor independently from the main review cursor across refreshes. The tabbar should render compact mode labels such as `F[i]les 3` and `C[o]mments 5`, with the selected tab highlighted.
- File sidebar content should group files by parent path, render the directory row before file rows, use box-drawing border/tree glyphs rather than ASCII separators, and include compact colored Git status letters (`A`, `M`, `D`, `R`, `U`, `B`) plus added, removed, and net-delta line counts.
- Comment sidebar content should use the same Rust-owned box-drawing renderer as inline comments, including compact open/resolved status glyphs, collapsed counts, title/summary footers, anchor-status footers, truncation, and highlight spans.
- From either the review buffer or the sidebar, normal-mode `i`/`I` opens or focuses the `F[i]les` sidebar mode, and `o`/`O` opens or focuses the `C[o]mments` sidebar mode. In the sidebar, normal-mode `p` focuses the main Peers diff/review view.
- The sidebar should highlight the file or thread corresponding to the current review-buffer cursor so the user can see which sidebar item they are currently inspecting. This inspected-item highlight should use a different background than the sidebar cursor-line highlight.
- Review-owned shortcut actions should work from both the review buffer and sidebar when the selected row has enough metadata: `c` opens the composer or replies to the selected thread/comment, visual `c` opens a range comment for selected diff lines, `dd` deletes the selected editable comment with confirmation, `dt` deletes the selected thread with confirmation, `A` asks the configured agent to do a full review of all currently open threads, `R` asks the configured agent to respond to and resolve the selected thread with code changes, `C` asks the configured agent to comment on the selected thread without code changes, `S` shows the commit summary and asks the configured agent to inspect, verify, and commit the current working tree changes, `D`/`U` navigate to the next/previous thread, `X` toggles collapsed/expanded file state, `r` toggles the selected thread between resolved and reopened, and `x` toggles collapsed/expanded thread state. Sidebar actions proxy through the owning review buffer rather than running a separate LSP client.

- File collapse state is durable repo-local UI state keyed by path, stored separately from canonical comment payloads. If a user collapses a path such as `Cargo.lock`, future Peers review renders should keep that file block collapsed until the user toggles it open again, even when the diff hunks for that file change.
- Comment jumps into a collapsed file, including sidebar jumps and review-buffer `D`/`U` thread navigation, should temporarily expand that file so the target thread can be inspected. The durable collapsed state is preserved, and the file should automatically collapse again once the review cursor leaves that file block.
- In the sidebar, `<CR>` jumps the main review window to the selected file/thread/comment row. File rows keep focus in the sidebar; comment/thread rows move focus to the review window. `q` hides the sidebar.
- Sidebar buffers are read-only review UI surfaces, so normal-mode insert keys such as `i`, `o`, and related uppercase variants may be remapped there.
- Limitation: Neovim does not provide a native way to make a focusable sidebar window delegate arbitrary buffer/window commands to its attached review window. Because the sidebar uses `winfixbuf`, buffer-switching commands issued while focused in the sidebar may be blocked or no-op depending on the caller. Users should press `p` to return to the review window or `q` to close the sidebar before normal buffer/window operations.

Cursor and viewport stability during live updates:

- Re-rendering the synthetic review buffer must preserve the user's semantic position, not merely the same Neovim buffer row number.
- Before replacing buffer lines, capture a cursor anchor from the current row metadata. Prefer stable identities in this order: comment id, thread id plus comment-relative row, source path/side/source line and column, hunk/file path plus relative offset, then raw buffer row as a last resort.
- After rendering the new projection, restore the cursor to the best matching row for that semantic anchor. If the exact comment, thread, or source line still exists, move the cursor there even if inserted/removed lines changed its Neovim row number.
- If the exact row no longer exists, fall back to the nearest meaningful context in the same thread, then same source line/range, then same hunk, then same file header, then the nearest surviving row by old render order.
- Preserve the viewport around the restored semantic row where practical, so live updates do not jump the user away from the code/comment they were reading.
- Composer windows should pin to the semantic anchor they were opened from. If that anchor disappears during a live update, keep the draft text and move the composer to the nearest fallback row with a visible warning rather than silently closing or submitting stale context. Peers-owned composers may pause live refresh while the user is typing, but third-party floating windows must not pause refresh globally; LSP progress UIs such as fidget should not delay diff updates until the underlying language server finishes.
- Direct user-initiated navigation should win over delayed refreshes. If a refresh completes after the user has moved the cursor, do not restore an older cursor anchor over the newer position.

For current-side rows, Neovim should open the real file in a hidden source buffer so the user's normal Tree-sitter and language server setup attaches. Hidden buffers are used first for syntax mirroring, and Peers can then proxy LSP-like requests from the `peersdiff` buffer to that hidden real buffer. The bundled plugin may transparently wrap normal `vim.lsp.buf` entry points while the active buffer is a Peers review buffer so existing user mappings keep working, but source code actions must remain disabled so review-owned actions stay predictable. Deleted/base-side rows may initially provide Peers metadata only; hidden base-version buffers can be added later as a best-effort enhancement.

Because an LSP server cannot directly manipulate Neovim buffers, the Neovim integration will likely also need a small Neovim attachment layer. Keep that layer thin:

- Target Neovim 0.12 as the supported editor version for the bundled plugin.
- Lua provides the `:Peers` bootstrap and attaches the review buffer to the Peers session.
- The plugin should be installable from this repository as a normal Neovim runtime package with top-level `plugin/` and `lua/` files.
- The bootstrap should check the `pid` in `session.json`, discard stale session files when the process is gone, start a fresh Peers session, and stop or avoid stale `peersdiff` LSP clients so an old dead port is not reused after a session restart.
- If Neovim starts the Peers session, quitting Neovim should stop that child process by default. Sessions started outside Neovim must not be stopped by the plugin.
- If Peers needs to update Neovim buffers directly, the single Peers session process may connect to Neovim over Msgpack-RPC and use `nvim-rs`.
- Do not create a second long-lived Rust worker process for Neovim. The Peers session process should remain the single owner.

Realtime updates are required in Neovim too. Manual refresh is only a fallback when realtime is disabled or broken.

Neovim commands should stay small:

```vim
:Peers diff
:Peers diff cached
:Peers diff all
:Peers review
:Peers review main
:Peers review main HEAD
:Peers comment
:Peers agent <prompt>
:Peers stop
```

Most daily review actions should be available through normal LSP hover, definition, references, diagnostics, document symbols, and code actions.

## Archived Web Layout

The web frontend has been removed while the Neovim-first repo-scoped provider stabilizes. The notes below are archived design context only, not active implementation guidance.

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
- `Commits`: shown only for branch review mode, not for working-tree, cached, or all-changes diff modes.

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
- Accept agent comment.
- Decline agent comment.
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
- Delete comment should be available from comment-row code actions for comments the current author can edit/delete, and must show the same warning before invalidating later dependent activity.
- Editing or deleting a user comment invalidates later dependent activity in that thread.
- Dependent activity includes following agent comments, following agent-created replies, and later resolved/reopened status changes that happened after the edited/deleted comment.
- Before applying an edit/delete that would invalidate later activity, show a confirmation warning that those later comments/status changes will be removed from the visible thread state.
- After confirmation, the UI should remove the invalidated later activity from the visible thread and from generated review summaries/agent context.
- Because storage is append-only, do not physically rewrite old JSONL lines. Record edit/delete/invalidation events and let derived state hide the invalidated activity.

Agent comment accept/decline:

- `Accept agent comment` and `Decline agent comment` are review-owned actions, exposed primarily as Neovim code actions on agent-authored comment rows.
- They apply only to comments authored by `agent`; they should not appear on human-authored comments or on comments already accepted/declined unless the UI offers an explicit change-disposition action later.
- Accept records human acknowledgement that the agent feedback is valid or worth acting on. It should keep the thread unresolved unless the user separately resolves it.
- Decline records human rejection of the agent feedback. The action should prompt for an optional reason and append it either as transition metadata or as a normal human reply linked to the decline.
- If declining the last actionable agent comment in a thread, the UI may offer a combined `Decline and resolve` action, but it must append both `agent_comment_declined` and `thread_resolved` so history remains explicit.
- Accepted unresolved agent comments should remain visible in `peers thread list --status open --context` until the thread is resolved.
- Declined comments should be hidden from default open/actionable agent context, but remain visible in complete/global listings and thread history.
- Reopening a resolved thread does not erase prior accept/decline decisions. A later agent reply starts pending again and can be accepted or declined independently.
- Future patch/suggestion support may let `Accept agent comment` apply a machine-readable edit before recording acceptance. Until then, acceptance is a disposition marker only, not a code-changing operation.

Inline thread behavior:

- Existing threads render directly below their anchored line or range in both diff and full-file views.
- Multiple threads on the same line or range render in a stable order by creation time.
- Resolved threads may be collapsed by default, but unresolved threads should be visible without opening a side panel.
- Collapsed threads should show a compact header with status/count metadata and the thread title when present, so the row remains useful as a context reminder.
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

Comment search should search comments relevant to the current projection by default, regardless of the file visibility filter. It should also support a repo/global mode that searches all non-pruned repo-scoped comments. Selecting an anchored or file-level comment in a currently hidden unchanged file should open the file directly and indicate that it is outside the current file filter. Selecting a repo/review-level comment should open the conversation/global comment surface when that UI exists.

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

## Performance Profiling

Use structured Rust tracing before guessing at realtime refresh performance. Run Peers with `PEERS_LOG=peers=info` or `RUST_LOG=peers=info` to emit span-close timings for provider projection, diff loading, anchor indexing/relocation, LSP rendering, and realtime publish paths. Use `PEERS_LOG=peers=debug` when diagnosing update spam; update broadcasts log their kind and sequence at debug level. Backend traces are written to `.peers/backend.log` for repo commands, so use `tail -f .peers/backend.log` while reproducing slow refreshes.

Use sampling profilers for CPU flamegraphs rather than custom timers. The expected workflow is to start a Peers session with symbols enabled, trigger the slow refresh path from Neovim, and capture the running `peers` process with `cargo flamegraph`, `perf`, or an equivalent platform profiler. Rust tracing identifies the slow logical phase; the flamegraph identifies the hot functions inside that phase.

Neovim-side work remains outside Rust tracing. Lua may use lightweight `PEERS_TIMING=1` instrumentation for render RPC roundtrip, buffer application, sidebar update, and viewport Tree-sitter mirroring timings. Plugin timings are appended to `.peers/nvim.log`, so use `tail -f .peers/nvim.log` alongside the backend log.

## Feature Status

Use this section as the short source of truth for implementation progress. Update it whenever a feature moves materially closer to or farther from the spec.

Statuses:

- `Complete`: implemented and believed to follow the current spec.
- `Partial`: implemented enough to use or preview, but known gaps remain.
- `Planned`: specified, but not meaningfully implemented.
- `Low priority`: implemented or specified, but intentionally deprioritized relative to current product direction.
- `Out of date`: implemented behavior exists but conflicts with the current spec.

Current status:

| Feature | Status | Notes |
| --- | --- | --- |
| Project rename to Peers | Complete | CLI/package/docs use `peers`, `.peers`, and `PEERS_*`. |
| CLI skeleton | Partial | Commands exist, `peers skill` prints an agent workflow overview, `peers thread list` prints visible current threads, and hidden `peers session ...` commands launch repo-scoped local sessions. |
| Repo-scoped comment architecture | Partial | Canonical storage is now one repo-level event log plus per-thread/comment payload files. Projection relocation remains basic. |
| Review storage/event log | Partial | Append-only JSONL now records lightweight comment/thread actions under `.peers/events.jsonl`; payloads live under `.peers/threads/`. |
| Author detection and overrides | Complete | Git config, CLI flags, `PEERS_*` env vars, and explicit agent identity are implemented. |
| CLI comment operations | Partial | List/add/reply/edit/delete/resolve/reopen operate on repo-scoped state; richer projection filtering remains. |
| `peers clean` | Partial | Cleanup previews and archives resolved candidates with confirmation unless explicitly non-interactive; detached/hidden/age policies remain coarse. |
| Agent launcher/session wrapper | Partial | `peers agent codex`, `peers agent -- <command>`, `peers agent attach --addr ws://...`, and initial Neovim `:Peers agent <prompt>` invocation are implemented for loopback websocket Codex app-server sessions. Running `peers agent` without an explicit agent or passthrough command prints help. Rich comment-aware prompts and a generic agent trait remain. |
| Agent comment accept/decline | Planned | Agent-authored comments should expose accept/decline code actions and append explicit disposition events without conflating disposition with thread resolution. |
| Thread titles | Planned | Agent resolution should set a short thread title so collapsed resolved threads can show a useful reminder without expanding the full conversation. |
| Anchor relocation and visibility policy | Planned | Rich content/context anchors and aggressive unresolved vs conservative resolved visibility are specified but not implemented. |
| Generated review and agent context files | Partial | Basic generated files read repo-scoped state and replay hides invalidated dependent activity; richer projection context remains. |
| Git diff loading | Partial | Working tree, cached, all-changes, and branch targets load real Git diffs into the compact payload through gitoxide snapshots and `gix-diff` hunk generation. Rename detection is currently exact-content only, and richer normalization fixtures remain. |
| Arborium highlighting | Planned | Not implemented. |
| LSP RPC service | Partial | The active local RPC surface is `peersdiff` LSP plus custom Peers methods for rendering, mutations, thread collapse, source-buffer code actions, and agent engagement. |
| Realtime UI updates | Partial | The session broadcasts coarse `review_changed` and `diff_changed` updates through the in-process provider broadcaster, watches `.peers/events.jsonl`, `.peers/threads/`, and the repository tree with debounce plus polling fallback, and refreshes Neovim review buffers through the active `peersdiff` LSP session. |
| Neovim review mode | Partial | The local Peers session starts a `peersdiff` LSP endpoint using `tower-lsp-server`; hidden `peers session diff` and `peers session review` launch repo-scoped sessions, and Lua `:Peers` opens a full-focus synthetic review buffer from `.peers/session.json`. Rust serves `peers/renderReview` with rendered diff rows, row metadata, structural highlights, source-buffer decorations, document symbols for the review buffer, and an empty state when there are no changed files; Lua applies rows, mirrors viewport-scoped Tree-sitter highlights from hidden current-side source buffers, opens floating writable composers for add/reply/edit comment code actions, executes delete/resolve/reopen mutations, shows edit/delete invalidation confirmation through native Neovim prompts, masks file diffs when Neovim has unsaved changes in the corresponding source buffer, publishes diagnostics for those masked files, refreshes through the LSP session while a review session is active, attaches Peers to source buffers as code-actions-only, and proxies hover/definition/declaration/type-definition/implementation/references from mapped current-side rows into hidden source buffer LSP clients. Line comments render inline at their anchor row with range rails and source-buffer gutter rails. Broader relocation diagnostics remain. |
| Neovim sidebar | Partial | A right-side fixed `winfixbuf` sidebar is specified and initially implemented for visible files and comments; richer grouping, filtering, and presentation polish remain. |
| Neovim cursor stability | Planned | Live review-buffer refreshes should restore cursor and viewport by semantic row anchors such as comment id, thread id, or source path/line rather than raw Neovim line number. |
| Comment-context hunks | Planned | Open comments should render relevant unchanged regions even when the current Git diff is otherwise empty; `No file changes` should require no visible relevant comments. |
| Web review workspace | Removed | The frontend directory and TypeScript binding generator were removed during the Neovim-first provider refactor. Recover from Git history if a web surface becomes useful again. |
| Frontend review payload shape | Removed | The active contract is the Rust provider/Neovim projection path. |
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
| Packaging embedded frontend assets | Removed | Not applicable while the web frontend is removed. |

## Implementation Order

Current priority order:

1. Deepen review/diff projections over repo-scoped comments and current Git state.
2. Add rich content/context capture for new line and range comments.
3. Implement anchor relocation and placement classification: inline, file-level, repo-level, detached, hidden.
4. Apply visibility policy: unresolved comments surface aggressively; resolved comments hide aggressively as context drifts.
5. Render comment-context hunks for visible unresolved comments even when the Git diff is empty.
6. Refine CLI comment projection filters for `--scope view|repo|detached`.
7. Harden `peers clean` age, hidden, and detached candidate policies.
8. Add agent comment accept/decline disposition events, provider operations, CLI commands, and Neovim code actions.
9. Preserve Neovim cursor and viewport by semantic row anchors during live refreshes.
10. Retarget Neovim rendering and realtime updates to repo-scoped projections.
11. Improve Neovim diagnostics for detached/stale unresolved comments and cleanup candidates.
12. Keep generated `review.md` and CLI thread context output as views over repo-scoped projections.
13. Add Arborium highlighting only if it still matters after Neovim Tree-sitter mirroring covers the primary workflow.
14. Revisit a non-Neovim local RPC API only if a real client needs it.
