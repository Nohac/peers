# Peers

Peers is a local Git review tool for reviewing changes before committing or merging. It is intended to feel similar to a GitHub pull request review, but it runs locally and stores review comments inside the project so humans and AI agents can both work from the same feedback.

This project is in early development. See [spec.md](./spec.md) for the full implementation plan.

## Goals

- Review unstaged, staged, full working tree, and branch-range diffs.
- Open a local Neovim review session from the CLI.
- Select lines or ranges and create comment threads.
- Add, edit, delete, reply to, resolve, and reopen comments.
- Store reviews in an append-friendly local format.
- Make review comments easy for AI agents to read and update.

## Planned Stack

- Rust backend
- Tokio async runtime
- Clap derive for the CLI
- gitoxide / `gix` for Git operations
- `tower-lsp-server` for the planned Neovim `peersdiff` LSP
- `facet` and `facet-json` for serialization
- Arborium for server-side syntax highlighting

## Planned CLI

Learn the agent workflow:

```bash
peers skill
```

Check review state:

```bash
peers thread list
```

The bundled Neovim plugin starts hidden Peers session commands for current changes or branch reviews.


Create and manage threads:

```bash
peers thread list
peers thread list --status open
peers thread list --status complete
peers thread show thr_123 --context 8
peers thread show thr_123 --context 8 --evidence
peers thread add --path src/foo.rs --side new --lines 42:47 --body "This needs validation."
peers thread reply thr_123 --body "Fixed in the latest change."
peers thread resolve thr_123
```

Agent-oriented usage:

```bash
peers agent codex
peers agent -- codex --remote %addr
peers thread --agent "Codex (GPT-5)" add --path src/foo.rs --side new --lines 42:47 --body-file -
```

Inside Neovim, `:PeersAgent <prompt>` sends a prompt to the active Codex app-server session recorded in `.peers/agent-session.json`.

Neovim session usage starts a local Peers session for a repo-scoped projection and writes the `peersdiff` LSP endpoint that the Neovim integration can attach to.
The same connection details are written to `.peers/session.json` while the session is running.

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
  events.jsonl
  threads/
    thr_123/
      thread.json
      comments/
        cmt_123.json
  session.json
  review.md
```

`events.jsonl` is the canonical append-only action log. Thread/comment payload files hold bodies and anchors. `review.md` is a generated human-readable view.

## Development

The repository currently contains the Rust CLI/backend and bundled Neovim integration.

Rust commands:

```bash
cargo check
cargo run
```
