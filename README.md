# Committeer

Committeer is a local Git review tool for reviewing changes before committing or merging. It is intended to feel similar to a GitHub pull request review, but it runs locally and stores review comments inside the project so humans and AI agents can both work from the same feedback.

This project is in early development. See [spec.md](./spec.md) for the full implementation plan.

## Goals

- Review unstaged, staged, full working tree, and branch-range diffs.
- Open a local review UI from the CLI.
- Select lines or ranges and create comment threads.
- Add, edit, delete, reply to, resolve, and reopen comments.
- Store reviews in an append-friendly local format.
- Make review comments easy for AI agents to read and update.

## Planned Stack

- Rust backend
- Tokio async runtime
- Clap derive for the CLI
- gitoxide / `gix` for Git operations
- Vox for local RPC
- `facet` and `facet-json` for serialization
- Arborium for server-side syntax highlighting
- TanStack Start frontend
- React Query
- shadcn/ui and Tailwind CSS

## Planned CLI

Review current changes:

```bash
committeer diff
committeer diff --cached
committeer diff --all
```

Review a branch against a base branch:

```bash
committeer review
committeer review --base main
committeer review --base origin/main
```

Create and manage comments:

```bash
committeer comment add --path src/foo.rs --side new --lines 42:47 --body "This needs validation."
committeer comment reply thr_123 --body "Fixed in the latest change."
committeer comment resolve thr_123
```

Agent-oriented usage:

```bash
committeer --agent comment add --path src/foo.rs --side new --lines 42:47 --body-file -
committeer agent-context
```

## Review Storage

Reviews are planned to be stored in the reviewed repository:

```text
.committeer/
  current
  reviews/
    rev_20260528_121530_a1b2c3/
      events.jsonl
      review.md
      agent-context.md
```

`events.jsonl` is the canonical append-only review log. Markdown files are generated for humans and agents.

## Development

The repository currently contains a Rust crate scaffold and a TanStack Start frontend in `frontend/`.

Frontend commands:

```bash
cd frontend
bun install
bun run dev
bun run fmt:check
bun run lint
bun run ts:check
```

Frontend tooling:

- `oxfmt` formats TypeScript, TSX, and related frontend files.
- `oxlint` handles frontend linting.
- `tsgo --noEmit` handles TypeScript checking.

Rust commands:

```bash
cargo check
cargo run
```
