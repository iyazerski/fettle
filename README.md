<div align="center">

<img src="docs/screenshots/icon.png" width="112" alt="Fettle app icon">

# Fettle

**Fix grammar and translate text in any Mac app with one shortcut.**

Select text, press <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd>, and click **Fix** or **Translate**.<br>
A free, open-source AI writing assistant for macOS, and a lightweight alternative to Grammarly.

[![Download Fettle.dmg](https://img.shields.io/github/v/release/iyazerski/fettle?label=download&color=3b6cf6)](https://github.com/iyazerski/fettle/releases/latest/download/Fettle.dmg)
[![macOS](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20silicon-black?logo=apple)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org)
[![Buy me a coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/iyazerski)

**Fix**<br>
<img src="docs/demo/fix.gif" width="600" alt="Selecting a Gmail draft, pressing the shortcut, and clicking Fix: Fettle shows each change, with removed words struck through in red and new words in green, then Replace puts the fixed text back into Gmail">

**Translate**<br>
<img src="docs/demo/translate.gif" width="600" alt="Selecting a Gmail draft, pressing the shortcut, and picking Spanish next to Translate: Fettle translates the English reply, then Replace puts it back into Gmail">

</div>

## Why Fettle

- **Works in every app.** Mail, Slack, Notes, your browser, your code editor. If you can select the text, Fettle can fix it.
- **See every change.** Fixes show up word by word, like track changes.
- **Translate into 40 languages.** Fettle keeps the tone, formatting, names, links and code. Star the languages you use most to keep them at the top.
- **Paste it back in one keystroke.** <kbd>⌘</kbd><kbd>↩</kbd> replaces your selection with the result. Your clipboard stays as it was.
- **Use your ChatGPT plan or your own API.** Sign in with ChatGPT, or point Fettle at any OpenAI-compatible API, such as OpenAI, OpenRouter, or a local model in Ollama or LM Studio.
- **Native and tiny.** Written in Rust, a 10 MB download, about 12 MB of memory when idle, and no CPU use until you call it.
- **Private by design.** No account, no telemetry, no servers of ours. Your text goes only to the model you pick.
- **Free and open source** under the MIT license.

## Install

For macOS 13 or later on Apple silicon (M1 and newer). Pick one:

- [**Download**](https://github.com/iyazerski/fettle/releases/latest/download/Fettle.dmg) and install the latest release.
- **Homebrew:** `brew install --cask iyazerski/tap/fettle`

### First launch

Fettle isn't notarized by Apple yet, so macOS blocks it the first time you open it. To allow it:

1. Open Fettle from Applications. macOS says it can't verify the app. Click **Done**.
2. Open **System Settings > Privacy & Security** and scroll down to **Security**.
3. Click **Open Anyway** next to the message about Fettle, and confirm.

## FAQ

<details>
<summary><b>Why the name?</b></summary>

To fettle is an old English word for to tidy up or put right, and "in fine fettle" means in good shape. That's what Fettle does to your text.

</details>
<details>
<summary><b>Is Fettle a Grammarly alternative?</b></summary>

Yes, for the core job: fixing grammar, spelling and phrasing in any app, with a clear view of what changed. It doesn't underline mistakes as you type. You select the text and press the shortcut when you want a check.

</details>
<details>
<summary><b>Which apps does it work in?</b></summary>

Any app where you can select text and copy it with <kbd>⌘</kbd><kbd>C</kbd>. **Replace** needs a text field you can type into.

</details>
<details>
<summary><b>Which AI models can I use?</b></summary>

With a ChatGPT plan, the models your plan offers through Codex. With an API, any model that the API's `/v1/models` endpoint lists.

</details>
<details>
<summary><b>Does it cost anything?</b></summary>

Fettle is free. The model is billed by your provider: it's part of your ChatGPT plan, or charged by your API provider.

</details>
<details>
<summary><b>Does it run on Intel Macs, Windows or Linux?</b></summary>

Not yet. Fettle is built for macOS on Apple silicon.

</details>
<details>
<summary><b>Is it safe to grant Accessibility?</b></summary>

Fettle uses it only to press <kbd>⌘</kbd><kbd>C</kbd> and <kbd>⌘</kbd><kbd>V</kbd> when you call it. The code is open, so you can check.

</details>

## Build from source

Fettle is a native Rust app built on the [Rune](https://github.com/anomalyco/rune) UI framework.

```sh
make install   # build Fettle.app and put it in /Applications
make test
```

Pre-commit hooks run with [prek](https://github.com/j178/prek): `prek install`.

## Privacy

- Fettle has no account and no analytics, and it has no server of its own.
- The text you fix or translate goes only to the model provider you chose: OpenAI when you sign in with ChatGPT, or the API URL you entered.
- Fettle restores your clipboard after it copies and pastes.

## License

[MIT](LICENSE) © Ihar Yazerski. Fettle includes open-source software under its own licenses: see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). Issues and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

Fettle isn't affiliated with or endorsed by OpenAI or Grammarly. ChatGPT and Grammarly are trademarks of their owners. Signing in with ChatGPT uses the same sign-in as OpenAI's Codex, so it may stop working if OpenAI changes it. An API key works either way.
