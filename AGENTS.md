# Agent Entry Point

Peers is a local Git review tool. It has a Rust CLI/backend and bundled Neovim integration.

Start here:

- [README.md](./README.md): project overview, planned stack, basic commands.
- [spec.md](./spec.md): product and implementation plan.
- [CONTRIBUTING.md](./CONTRIBUTING.md): coding standards, architecture rules, and testing policy.

Current shape:

- Rust source: `src/`
- Neovim plugin source: `lua/` and `plugin/`
- Review storage target: `.peers/` inside reviewed repos

Implementation note:

- Check [CONTRIBUTING.md](./CONTRIBUTING.md) before making implementation decisions.
- Never commit changes unless the project owner has reviewed the work and explicitly told you to commit.
