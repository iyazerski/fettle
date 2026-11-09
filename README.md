<div align="center">

<img src="docs/screenshots/icon.png" width="112" alt="Fettle app icon">

# Fettle

**Fix grammar, rewrite and translate text in any Mac app with one shortcut.**

Select text, press <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd>, and click **Fix**, **Translate** or **Rewrite**.<br>
A free, open-source AI writing assistant for macOS that can run entirely on your Mac, and a lightweight alternative to Grammarly.

[![Download Fettle.dmg](https://img.shields.io/github/v/release/iyazerski/fettle?label=download&color=3b6cf6)](https://github.com/iyazerski/fettle/releases/latest/download/Fettle.dmg)
[![macOS](https://img.shields.io/badge/macOS-13%2B%20·%20Apple%20silicon-black?logo=apple)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org)
[![Buy me a coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/iyazerski)

**Fix**<br>
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/fix-dark.png">
  <img src="docs/screenshots/fix-light.png" width="540" alt="Fettle fixing the grammar of an email, with removed words struck through in red and new words in green">
</picture>

**Translate**<br>
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/translate-dark.png">
  <img src="docs/screenshots/translate-light.png" width="540" alt="Fettle translating a short English message into Spanish">
</picture>

</div>

## Why Fettle

- **Like Grammarly, but light, fast and free.** Fettle underlines mistakes as you type in Mail, Slack, Notes, your browser and most other apps. Click one to fix it. No account, no subscription, no browser extension. It's a 10 MB app that uses about 12 MB of memory when idle.
- **Like Google Translate or DeepL, but it sounds like a person wrote it.** Fettle translates into 40 languages the way a native speaker would say it, not word by word. Your tone, formatting and names stay as they are.
- **Write like a native in any language.** Fix catches grammar and spelling, and also swaps awkward or literally translated phrases for what a native speaker would actually say. Your text stays in your language, and your meaning and tone stay yours.
- **Get the tone right in one click.** Make an email more formal, a message friendlier, a long reply shorter, or a messy paragraph clearer.
- **Talk instead of type.** Click the mic and just speak. Fettle drops the ums, repeats and false starts, adds punctuation, and hands you a clean message. Built-in dictation types exactly what you said. Fettle writes what you meant.
- **Works where you write.** Select text in any app, press <kbd>⌃</kbd><kbd>⌥</kbd><kbd>E</kbd>, and put the result back with <kbd>⌘</kbd><kbd>↩</kbd>. No copying between tabs.
- **Private, and free with Apple Intelligence.** On macOS 26, Fettle can use Apple's on-device model. Your text never leaves your Mac, and there's nothing to pay. Want a bigger model? Use your ChatGPT plan, or OpenAI, OpenRouter, Ollama or LM Studio.
- **Open source.** MIT licensed, with no telemetry and no servers of ours.

## Install

For macOS 13 or later on Apple silicon (M1 and newer). The on-device model needs macOS 26 with Apple Intelligence turned on. Pick one:

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

Yes, for the core job: fixing grammar, spelling and phrasing in any app, with a clear view of what changed. Select the text and press the shortcut when you want a check. Or turn on **Underline mistakes** in Settings: when you pause typing, Fettle sends each paragraph you edited to the model and underlines what it would fix, and a click on an underline shows the fix. Each edited paragraph is a request, so this costs much more with a paid model. Only multi-line text boxes, such as message composers and documents, are checked, not address bars, search boxes or other one-line fields. Underlines show in apps that tell macOS where their text is on screen, which most native Mac apps do.

</details>
<details>
<summary><b>Which apps does it work in?</b></summary>

Any app where you can select text and copy it with <kbd>⌘</kbd><kbd>C</kbd>. **Replace** needs a text field you can type into.

</details>
<details>
<summary><b>Which languages does it support?</b></summary>

Fix and Rewrite work in any language the model knows, whatever language the app is in. Your text stays in the language you wrote it in. Translate goes into 40 languages. The app itself is in English, Spanish, French, German, Portuguese, Polish, Russian, Chinese, Japanese, Korean, Hindi and Arabic, and follows your Mac's language unless you pick one in Settings.

</details>
<details>
<summary><b>Which AI models can I use?</b></summary>

- **On this Mac:** Apple's on-device model, which comes with Apple Intelligence on macOS 26. It's free and private, but smaller, so it takes shorter texts and fewer languages.
- **ChatGPT plan:** the models your plan offers through Codex.
- **API:** any model that the API's `/v1/models` endpoint lists.

</details>
<details>
<summary><b>Why does Fettle say Apple Intelligence isn't ready?</b></summary>

Turn it on in **System Settings > Apple Intelligence & Siri**. Your Mac and Siri must use the same language, and Apple Intelligence must support it: English, French, German, Italian, Spanish, Portuguese, Japanese, Korean, Chinese, Danish, Dutch, Norwegian, Swedish, Turkish or Vietnamese. Right after you turn it on, macOS may need a while to download the model.

</details>
<details>
<summary><b>Does it cost anything?</b></summary>

Fettle and the on-device model are free. Other models are billed by your provider: they're part of your ChatGPT plan, or charged by your API provider.

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
- With **On this Mac**, your text never leaves your Mac.
- Otherwise, the text you fix or translate goes only to the model provider you chose: OpenAI when you sign in with ChatGPT, or the API URL you entered.
- Dictation uses Apple's speech recognition, on your Mac when it supports your language, else on Apple's servers.
- Your ChatGPT sign-in and API key are kept in the macOS Keychain.
- Fettle restores your clipboard after it copies and pastes.

## License

[MIT](LICENSE) © Ihar Yazerski. Fettle includes open-source software under its own licenses: see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). Issues and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

Fettle isn't affiliated with or endorsed by OpenAI or Grammarly. ChatGPT and Grammarly are trademarks of their owners. Signing in with ChatGPT uses the same sign-in as OpenAI's Codex, so it may stop working if OpenAI changes it. An API key works either way.
