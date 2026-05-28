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
- Arborium for server-side syntax highlighting.

Backend organization should be behavior-oriented:

- `cli.rs`: command parsing and command dispatch.
- `diff.rs`: review target resolution, diff loading, diff normalization, highlighting integration.
- `review.rs`: review creation, review metadata, review lifecycle, current review selection.
- `comments.rs`: event model, JSONL parsing/encoding, replay, comment commands, agent context rendering.
- `rpc.rs`: Vox service trait and RPC-specific request/response DTOs.
- `server.rs`: local HTTP server, Vox endpoint, token handling, static/frontend serving.
- `ui_assets.rs`: embedded frontend assets.

## Rust IO Boundaries

Filesystem/path code must be thin. Any meaningful behavior should operate on loaded data, buffers, readers, writers, cursors, or strings.

Prefer:

```rust
fn encode_event(event: &ReviewEvent) -> Result<String>;
fn replay_events(events: &[ReviewEvent]) -> Result<ReviewState>;
async fn parse_events_from_reader(reader: impl AsyncBufRead + Unpin) -> Result<Vec<ReviewEvent>>;
async fn render_agent_context(state: &ReviewState, out: impl AsyncWrite + Unpin) -> Result<()>;
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

## Review Storage

Canonical review data is append-only JSONL under `.committeer/reviews/<review-id>/events.jsonl`.

Generated files such as `review.md` and `agent-context.md` are views over the event log, not canonical state.

Agents should normally use CLI commands to add/reply/resolve comments, but the JSONL format must remain simple enough to inspect and append in emergencies.

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

## Frontend Architecture

Frontend stack:

- TanStack Start.
- TanStack Router.
- React Query.
- shadcn/ui-style primitives.
- Tailwind CSS.
- Geist and Geist Mono fonts.
- Zod for route/search params and general validation.

Frontend files should be split by behavior:

```text
frontend/src/features/
  review/
  diff/
  comments/
```

Avoid large files. Prefer one meaningful component per file.

Route and layout files should stay lean:

- They wire route data, search params, layout, and child components.
- They should not contain detailed row styling, diff rendering, or comment rendering logic.

Primitive components may use inline Tailwind freely and should follow shadcn conventions. Composition components should mostly arrange smaller components and keep Tailwind minimal.

## Routing

Use TanStack Router as the source of truth for app navigation.

- Shared app chrome belongs in route layouts, not duplicated in page components.
- Root-level shared UI such as the toolbar and quick access menu should live in the root route layout and render child routes through `<Outlet />`.
- Use `Link` and `useNavigate` for app navigation.
- Do not use `window.location.href` for navigation.
- Do not use raw `<a href>` for app-owned routes, hashes, or search-param changes.
- File views must be linkable.
- Comment focus must be URL-driven, not hidden component state.
- The default review route should be a scroll-through list of all changed files.
- Sidebar file links should route/hash-scroll to the matching file section.
- Unchanged files, when visible, should open the full-file route directly.
- Full-file view should be its own route.

Use zod for route search validation:

```ts
const searchSchema = z.object({
  comment: z.preprocess((value) => (typeof value === "string" ? value : undefined), z.string().optional()),
});
```

Prefer URL search params for state that should be linkable or reproducible:

- active comment
- selected file/full-file route
- view mode
- include unchanged files, if we want the filter to be shareable

If TanStack Router can represent it cleanly in route/search state, prefer that over global client state.

## Client State

Use local component state for state that is truly local.

Use URL state for linkable state.

If shared ephemeral UI state becomes necessary, prefer a small Zustand store over React context. Good candidates:

- quick access query/open state, if it needs to be controlled from multiple distant components
- sidebar collapsed/width state
- active line selection
- composer draft state

Do not use React context as a general state container. Context is acceptable for dependency injection or stable layout services, but not as the default answer to shared UI state.

For files, prefer a generic file data/provider layer that can expose filtered file lists:

```ts
getReviewFiles({ includeUnchangedFiles: boolean })
```

The quick access menu should consume files from that provider/filter layer, not from a sidebar-specific component.

## Hotkeys

Keyboard shortcuts should be implemented consistently.

- Do not hand-roll global `window`/`document` listeners in feature components.
- Prefer a small hotkey library if shortcut handling grows beyond simple local component handlers.
- The quick access shortcut is `Cmd+K` / `Ctrl+K`.
- Shortcut behavior should be available from the root layout on all routes.

## Styling

The UI should be quiet, dense, and work-focused. It can use GitHub-like review layout patterns, but it should not copy GitHub styling or colors.

- Use Tailwind CSS.
- Use only shadcn theme colors in component classes.
- Add new theme colors only for concrete repeated semantic needs.
- Do not introduce one-off hardcoded colors in component Tailwind classes.
- Use `text-success` for added-line stats and `text-destructive` for removed-line stats.
- Use Geist for normal UI text.
- Use Geist Mono for code, file paths, line labels, hashes, IDs, and other code-adjacent labels.
- Text must fit within controls on desktop and mobile.
- Use lucide icons for tool buttons where appropriate.
- No decorative gradients, orbs, or landing-page hero treatment for the app workspace.

## Frontend Tooling

Use the configured frontend tools in `frontend/package.json`:

```bash
bun run fmt
bun run fmt:check
bun run lint
bun run lint:fix
bun run ts:check
```

- `oxfmt` is the frontend formatter.
- `oxlint` is the frontend linter.
- `tsgo --noEmit` is the TypeScript checker.
- Keep generated and hand-written frontend code passing all three checks before considering frontend work complete.
- Prefer fixing lint and type errors in source instead of suppressing them.

Generated TanStack route tree files should remain generated. If formatting tools conflict with generated output, configure the formatter instead of manually editing generated files.

## Validation

Use zod for frontend validation:

- route params
- search params
- user input shapes
- RPC payload validation where helpful

Keep validation close to the boundary where data enters the app. Do not spread ad hoc `typeof` checks through rendering components.

## Verification

Before handing off frontend work, run:

```bash
cd frontend
bun run fmt:check
bun run lint
bun run ts:check
bun run build
```

Before handing off Rust work, run:

```bash
cargo fmt
cargo check
cargo test
```

