# Contributing

This project favors small, testable, boring pieces over broad abstractions. The product is a local review tool, so implementation choices should keep review workflows fast, linkable, and easy for humans and AI agents to inspect.

For product scope and feature planning, see [spec.md](./spec.md). This file is the coding and architecture standard.

## General Principles

- Prefer the existing project patterns over introducing new ones.
- Keep code functionally co-located. Do not create catch-all files such as `types.rs`, `types.ts`, `utils.ts`, or `components.tsx` unless the contents are genuinely small and shared.
- Keep files small enough to scan. Split a component or module when it starts mixing separate responsibilities.
- Make behavior testable even when we intentionally choose not to test every small function.
- Avoid cleverness around state, routing, IO, and concurrency. The app should be easy to reason about from the file tree.

## Rust Backend

Use a single Rust crate unless the codebase grows enough to justify a workspace.

Backend stack:

- Tokio runtime.
- Async APIs by default, including filesystem operations.
- Clap derive for CLI parsing.
- gitoxide / `gix` for Git operations.
- Vox for local RPC.
- `facet` and `facet-json` for serialization.
- `thiserror` for custom/domain errors.
- Arborium for server-side syntax highlighting.

Backend organization should be behavior-oriented:

- `cli.rs`: command parsing and command dispatch.
- `diff.rs`: review target resolution, diff loading, diff normalization, highlighting integration.
- `review.rs`: repo-scoped storage paths, payload IO, generated review artifacts, and repository discovery.
- `comments.rs`: event model, JSONL parsing/encoding, replay, comment commands, agent context rendering.
- `review_provider.rs`: cloneable async review provider shared by Vox RPC, Neovim LSP, and future local clients.
- `rpc.rs`: Vox service trait and token-checking wrapper around the review provider.
- `server.rs`: local session process, Vox endpoint, token handling, and Neovim LSP startup.

## Rust IO Boundaries

Filesystem/path code must be thin. Any meaningful behavior should operate on loaded data, buffers, readers, writers, cursors, or strings.

Prefer:

```rust
fn encode_event(event: &ReviewEvent) -> Result<String>;
fn replay_events(events: &[PeersEvent], payloads: &PayloadStore) -> Result<PeersState>;
async fn parse_events_from_reader(reader: impl AsyncBufRead + Unpin) -> Result<Vec<PeersEvent>>;
async fn render_agent_context(state: &PeersState, target: Option<&ReviewTarget>, out: impl AsyncWrite + Unpin) -> Result<()>;
```

Keep path wrappers small and uninteresting:

```rust
async fn load_events_file(path: &Path) -> Result<Vec<ReviewEvent>>;
async fn append_event_file(path: &Path, event: &ReviewEvent) -> Result<()>;
```

Do not put parsing, replay, validation, or transformation logic inside large functions that take `&Path`.

Apply the same rule to Git access:

- Keep gitoxide repository access thin.
- Normalize and transform already-loaded diff data in separate functions.

## Rust Async And State

- Use Tokio async file IO through `tokio::fs` and Tokio readers/writers.
- Keep CPU-only functions synchronous when they do not perform IO.
- Avoid blocking operations inside async request handlers.
- Minimize `Arc`, `Mutex`, and `RwLock`.
- Avoid `Arc<Mutex<_>>` especially.
- Prefer ownership, immutable snapshots, append-only storage, request-local state, or message flow.
- If a concurrent mutable map is truly needed, prefer a purpose-built structure such as `DashMap`.
- Avoid `tokio::spawn` unless there is a clear lifecycle reason.
- Prefer local async blocks with `tokio::select!`, `tokio::join!`, `futures::future::join_all`, or similar helpers.
- Define static protocol names, command names, scope names, labels, local host strings, and repeated messages as top-of-file constants instead of scattering hardcoded strings inline.
- Use `thiserror` for custom/domain error enums. Keep `anyhow` for application-level propagation and IO/context boundaries.

## Review Storage

Canonical review data is repo-scoped. Lightweight transition events are append-only JSONL under `.peers/events.jsonl`; thread and comment payloads live under `.peers/threads/<thread-id>/`.

Generated files such as `review.md` and `agent-context.md` are views over the event log, not canonical state.

Agents should normally use CLI commands to add/reply/resolve comments. Payload files and the JSONL action log must remain simple enough to inspect in emergencies.

## Testing

Keep tests minimal, but keep logic testable.

Test logic that can become subtle:

- JSONL event parse/encode roundtrip.
- Event replay.
- Anchor relocation.
- Agent context rendering if formatting becomes non-trivial.
- Diff normalization if it gains meaningful branching complexity.

Do not test:

- Filesystem wrappers.
- Path construction wrappers.
- Simple DTO mappings.
- Basic CLI flag plumbing.
- Basic component wrappers.

## Verification

Before handing off Rust work, run:

```bash
cargo fmt
cargo check
cargo test
```
