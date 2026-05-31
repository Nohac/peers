# Peers

Peers is a local Git review tool for reviewing changes before committing or merging. It is intended to feel similar to a GitHub pull request review, but it runs locally and stores review comments inside the project so humans and AI agents can both work from the same feedback.

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
- `tower-lsp-server` for the planned Neovim `peersdiff` LSP
- `facet` and `facet-json` for serialization
- Arborium for server-side syntax highlighting
- TanStack Start frontend
- React Query
- shadcn/ui and Tailwind CSS

## Planned CLI

Review current changes:

```bash
peers diff
peers diff --cached
peers diff --all
```

Review a branch against a base branch:

```bash
peers review
peers review --base main
peers review --base origin/main
```

Create and manage comments:

```bash
peers comment add --path src/foo.rs --side new --lines 42:47 --body "This needs validation."
peers comment reply thr_123 --body "Fixed in the latest change."
peers comment resolve thr_123
```

Agent-oriented usage:

```bash
peers --agent comment add --path src/foo.rs --side new --lines 42:47 --body-file -
peers agent-context
```

Neovim session usage:

```bash
peers nvim
peers nvim diff
peers nvim diff --cached
peers nvim diff --all
peers nvim review --base main --head HEAD
peers nvim --review rev_123
```

This starts the local Peers session for a review and prints the Vox and `peersdiff` LSP endpoints that a Neovim integration can attach to.
The same connection details are written to `.peers/reviews/<review-id>/session.json` while the session is running.

Neovim plugin usage:

The bundled Neovim plugin targets Neovim 0.12.

```lua
vim.pack.add({
  { src = "https://github.com/<owner>/peers", name = "peers" },
})

require("peers").setup({
  binary = "peers",
  stop_on_exit = true,
})
```

Then run:

```vim
:Peers diff
:Peers diff cached
:Peers diff all
:Peers review
:PeersReview
```

During local development from this checkout, point `binary` at Cargo:

```lua
require("peers").setup({
  binary = {
    "cargo",
    "run",
    "--manifest-path",
    "/home/jonas/Development/Rust/committeer/Cargo.toml",
    "--",
  },
  stop_on_exit = true,
})
```

## Review Storage

Reviews are planned to be stored in the reviewed repository:

```text
.peers/
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
