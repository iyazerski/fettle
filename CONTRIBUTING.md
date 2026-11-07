# Contributing

Thanks for helping with Quill. Bug reports, ideas and pull requests are all welcome.

## Report a bug or ask for a feature

[Open an issue](https://github.com/iyazerski/quill/issues/new/choose) and pick a template. For questions, use [Discussions](https://github.com/iyazerski/quill/discussions). For security problems, follow [SECURITY.md](SECURITY.md) instead.

## Make a change

You need macOS on Apple silicon and [Rust](https://rustup.rs).

```sh
make install   # build Quill.app and put it in /Applications
make test
```

A local build is signed ad hoc, so macOS asks for Accessibility again after each install.

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

By contributing, you agree that your work is released under the [MIT License](LICENSE).
