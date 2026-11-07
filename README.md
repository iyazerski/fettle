<div align="center">

<img src="docs/screenshots/icon.png" width="112" alt="Quill app icon">

# Quill

**Fix grammar and translate text in any Mac app with one shortcut.**

Select text, press <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd>, and click **Fix** or **Translate**.<br>
A free, open-source AI writing assistant for macOS, and a lightweight alternative to Grammarly.

[![Latest release](https://img.shields.io/github/v/release/iyazerski/quill?label=download&color=3b6cf6)](https://github.com/iyazerski/quill/releases/latest)
[![macOS](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20silicon-black?logo=apple)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/fix-dark.png">
  <img src="docs/screenshots/fix-light.png" width="540" alt="Quill fixing the grammar of an email, with removed words struck through in red and new words in green">
</picture>

</div>

## Why Quill

- **Works in every app.** Mail, Slack, Notes, your browser, your code editor. If you can select the text, Quill can fix it.
- **See every change.** Fixes show up word by word, like track changes. Removed words are struck through in red, new words are green.
- **Translate into 33 languages.** Star the ones you use most to keep them at the top.
- **Paste it back in one keystroke.** <kbd>⌘</kbd><kbd>↩</kbd> replaces your selection with the result. Your clipboard stays as it was.
- **Use your ChatGPT plan or your own API.** Sign in with ChatGPT, or point Quill at any OpenAI-compatible API, such as OpenAI, OpenRouter, or a local model in Ollama or LM Studio.
- **Native and tiny.** Written in Rust, a 10 MB download, about 12 MB of memory when idle, and no CPU use until you call it.
- **Private by design.** No account, no telemetry, no servers of ours. Your text goes only to the model you pick.
- **Free and open source** under the MIT license.

## Translate

<div align="center">
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/translate-dark.png">
  <img src="docs/screenshots/translate-light.png" width="540" alt="Quill translating a short English message into Spanish">
</picture>
&nbsp;
<img src="docs/screenshots/languages-light.png" width="540" alt="Quill's language list, with favourite languages starred at the top">
</div>

Pick a language from the list next to **Translate**. Quill asks the model to keep the tone, formatting, names, links and code, and to leave text alone if it's already in that language.

Languages: Amharic, Arabic, Belarusian, Bengali, Chinese (Simplified), English, French, German, Gujarati, Hausa, Hindi, Indonesian, Italian, Japanese, Javanese, Kannada, Korean, Marathi, Persian, Polish, Portuguese, Punjabi, Russian, Spanish, Swahili, Tagalog, Tamil, Telugu, Thai, Turkish, Ukrainian, Urdu, Vietnamese.

## How it works

1. **Select** text in any app.
2. **Press** <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd>. Quill copies the selection and opens a small floating window at the top of the screen.
3. **Fix** (<kbd>⌘</kbd><kbd>1</kbd>) corrects grammar, spelling, punctuation and awkward phrasing, without rewriting what's already fine. **Translate** (<kbd>⌘</kbd><kbd>2</kbd>) writes it in the language you picked.
4. **Replace** (<kbd>⌘</kbd><kbd>↩</kbd>) pastes the result over your selection, or **Copy** (<kbd>⇧</kbd><kbd>⌘</kbd><kbd>C</kbd>) puts it on the clipboard.

You can also type or paste text straight into the window.

## Install

For macOS 13 or later on Apple silicon (M1 and newer). Run this in Terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/iyazerski/quill/main/install.sh | sh
```

This installs `Quill.app` into `/Applications` and starts it. To pin a release, set `QUILL_VERSION=v0.1.N`.

You can also download `Quill.zip` from the [latest release](https://github.com/iyazerski/quill/releases/latest). The app isn't notarized yet, so macOS blocks a copy downloaded in a browser. To allow it, run `xattr -dr com.apple.quarantine /Applications/Quill.app`.

**Updates:** click **Check for updates** in Settings. Quill downloads the new version, checks it, and restarts. Every release is signed with the same certificate, so macOS keeps the Accessibility permission after an update.

## Set up

Quill opens its settings on the first start:

1. **Model.** Choose one:
   - **ChatGPT plan:** sign in with your ChatGPT account, the same way Codex does.
   - **OpenAI-compatible API:** enter the base URL (up to `/v1`) and a key.

   Then pick a model from the list and how much it should reason. Reasoning off is the fastest.
2. **Accessibility.** Click **Allow…** and turn Quill on. Quill presses <kbd>⌘</kbd><kbd>C</kbd> and <kbd>⌘</kbd><kbd>V</kbd> for you to read the selection and paste the result, and macOS asks for this permission first.
3. **Appearance.** System, Light or Dark.
4. **Shortcut.** Click the field and press a new key combination to change it.
5. **Start at login.** Keeps Quill ready in the background.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd> | Open Quill with the selected text (configurable) |
| <kbd>⌘</kbd><kbd>1</kbd> | Fix |
| <kbd>⌘</kbd><kbd>2</kbd> | Translate |
| <kbd>⌘</kbd><kbd>↩</kbd> | Replace the selection with the result |
| <kbd>⇧</kbd><kbd>⌘</kbd><kbd>C</kbd> | Copy the result |
| <kbd>⌘</kbd><kbd>,</kbd> | Settings |
| <kbd>Esc</kbd> | Hide |

Drag the window by its title row to move it.

## Privacy

- Quill has no account and no analytics, and it has no server of its own.
- The text you fix or translate goes only to the model provider you chose: OpenAI when you sign in with ChatGPT, or the API URL you entered.
- Quill also talks to GitHub, but only when you click **Check for updates**.
- Quill restores your clipboard after it copies and pastes.
- Settings, including your sign-in token or API key, are saved in `~/Library/Application Support/Quill/settings.json`. Only your user account can read the file.

## FAQ

**Is Quill a Grammarly alternative?**
Yes, for the core job: fixing grammar, spelling and phrasing in any app, with a clear view of what changed. It doesn't underline mistakes as you type. You select the text and press the shortcut when you want a check.

**Which apps does it work in?**
Any app where you can select text and copy it with <kbd>⌘</kbd><kbd>C</kbd>. **Replace** needs a text field you can type into.

**Which AI models can I use?**
With a ChatGPT plan, the models your plan offers through Codex. With an API, any model that the API's `/v1/models` endpoint lists.

**Does it cost anything?**
Quill is free. The model is billed by your provider: it's part of your ChatGPT plan, or charged by your API provider.

**Does it run on Intel Macs, Windows or Linux?**
Not yet. Quill is built for macOS on Apple silicon.

**Is it safe to grant Accessibility?**
Quill uses it only to press <kbd>⌘</kbd><kbd>C</kbd> and <kbd>⌘</kbd><kbd>V</kbd> when you call it. The code is open, so you can check.

## Build from source

Quill is a native Rust app built on the [Rune](https://github.com/anomalyco/rune) UI framework.

```sh
make install   # build Quill.app and put it in /Applications
make test
```

Pre-commit hooks run with [prek](https://github.com/j178/prek): `prek install`.

## License

[MIT](LICENSE) © Ihar Yazerski. Issues and pull requests are welcome.
