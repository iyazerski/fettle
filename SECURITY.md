# Security

Quill has Accessibility access, so it can press keys in other apps, and it keeps your ChatGPT sign-in or API key. Please report anything that could misuse either.

## Report a vulnerability

Report it privately through [GitHub's vulnerability reporting](https://github.com/iyazerski/quill/security/advisories/new). Please don't open a public issue.

Include what you found, how to reproduce it, and the Quill version (shown at the bottom of Settings). You'll get an answer within a week.

## Supported versions

Only the latest release gets fixes. Quill updates itself from **Check for updates** in Settings.

## How releases are protected

- Every release is built by [GitHub Actions](.github/workflows/release.yml) from the public source.
- Every release is signed with the same certificate. Quill's updater and `install.sh` refuse a download that isn't signed with it.
- Quill isn't notarized by Apple yet.
