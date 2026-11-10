# Contributing

Thanks for helping with Fettle. Bug reports, ideas and pull requests are all welcome.

## Report a bug or ask for a feature

[Open an issue](https://github.com/iyazerski/fettle/issues/new/choose) and pick a template. For questions, use [Discussions](https://github.com/iyazerski/fettle/discussions). For security problems, follow [SECURITY.md](SECURITY.md) instead.

## Make a change

You need macOS on Apple silicon and [Rust](https://rustup.rs).

```sh
make dev       # build Fettle Dev.app and start it beside your Fettle
make test
make install   # build Fettle.app and put it in /Applications
```

Use `make dev` to try a change. Fettle Dev is a debug build with its own name, settings, sign-in and shortcut (⌃⌥⇧E), so it doesn't touch the Fettle you use every day. Allow it in Accessibility once: it's signed with a local identity, so macOS keeps the grant across rebuilds. See [AGENTS.md](AGENTS.md) for details.

`make install` signs ad hoc, so macOS asks for Accessibility again after each install.

Before you open a pull request:

1. Install the hooks with [prek](https://github.com/j178/prek): `prek install`. They run `cargo fmt`, `cargo check` and `cargo clippy`.
2. Run `make test`.
3. If you changed dependencies, run `make licenses` and commit `THIRD_PARTY_LICENSES.md`. It needs [cargo-about](https://github.com/EmbarkStudios/cargo-about).
4. Keep pull requests small and focused on one change.

## Code style

- Follow the code next to your change.
- Give every item a doc comment, and every `unsafe` block a `// SAFETY:` comment.
- Use comments to say why, not what.

Every merge to `main` that changes the app ships a release to all users, so `main` must always work.

By contributing, you agree that your work is released under the [MIT License](LICENSE). Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
