# Quill

Select text in any Mac app, press a shortcut, and **improve** or **translate** it in a small floating window.

- **Improve** fixes grammar, spelling and awkward phrasing, and shows what changed word by word.
- **Translate** writes the text in the language you pick. Star the languages you use most to keep them at the top of the list.
- **Replace** pastes the result over the selection. **Copy** puts it on the clipboard.

Quill is a native Rust app built on [Rune](https://github.com/anomalyco/rune). It's small, starts fast, and sits hidden until you press the shortcut.

## Install

macOS on Apple silicon:

```sh
curl -fsSL https://raw.githubusercontent.com/iyazerski/quill/main/install.sh | sh
```

This installs `Quill.app` into `/Applications` and starts it. To update, click **Check for updates** in Settings, or run the command again. To pin a release, set `QUILL_VERSION=v0.1.N`.

Releases are signed with the same certificate each time, so macOS keeps the Accessibility permission after an update.

You can also download `Quill.zip` from the [latest release](https://github.com/iyazerski/quill/releases/latest). The app isn't notarized, so macOS blocks a copy downloaded with a browser. To allow it, run `xattr -dr com.apple.quarantine /Applications/Quill.app`.

## Set up

When Quill starts, it opens its settings:

1. **Model.** Pick one of these:
   - **ChatGPT plan.** Sign in with your ChatGPT account, the same way Codex does.
   - **OpenAI-compatible API.** Enter the base URL (up to `/v1`) and a key.

   Then pick a model from the list and the reasoning level.
2. **Accessibility.** Click **Allow…** and turn Quill on. Quill presses ⌘C and ⌘V to read the selection and paste the result, and macOS asks for this permission first.
3. **Appearance.** System, Light or Dark.
4. **Shortcut.** The default is ⌃⌥E. Click the field and press another chord to change it.
5. **Start at login.** Turn this on to keep Quill running in the background.

## Use

Select text and press the shortcut. Drag the window by its title row. In the window:

| Key | Action |
| --- | --- |
| ⌘1 | Improve |
| ⌘2 | Translate |
| ⌘↩ | Replace the selection with the result |
| ⇧⌘C | Copy the result |
| ⌘, | Settings |
| Esc | Hide |

Quill puts your clipboard back after it copies and pastes.

The settings, including tokens, are saved in `~/Library/Application Support/Quill/settings.json`. Only you can read the file.

## Develop

```sh
make install   # build Quill.app and put it in /Applications
make test
```

Pre-commit hooks run with [prek](https://github.com/j178/prek): `prek install`.
