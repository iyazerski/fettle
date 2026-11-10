# Agent notes

## Try a change in the running app

Use Fettle Dev, never `make install`. `make install` replaces the user's own `/Applications/Fettle.app`.

```sh
make dev   # debug build, bundle target/debug/Fettle Dev.app, restart it
```

Fettle Dev is any debug build (`DEV` in `src/main.rs`). It runs beside the installed Fettle and shares nothing with it:

| | Fettle | Fettle Dev |
|---|---|---|
| App | `/Applications/Fettle.app` | `target/debug/Fettle Dev.app` |
| Process | `fettle` | `fettle-dev` |
| Bundle ID, keychain service, login item | `dev.iyazerski.fettle` | `dev.iyazerski.fettle.dev` |
| Settings and lock | `~/Library/Application Support/Fettle/` | `~/Library/Application Support/Fettle Dev/` |
| Default shortcut | ⌃⌥E | ⌃⌥⇧E |
| Menu bar icon | sparkles | hammer |

- Its output goes to `target/debug/fettle-dev.log`.
- New settings start on Apple's on-device model, so no sign-in is needed. It needs Apple Intelligence turned on.
- It doesn't update itself. Rebuild with `make dev`.
- Stop it with `pkill -x fettle-dev`. Never run `pkill -x fettle` or touch the installed app's settings or keychain items.
- To start over from the welcome steps: `pkill -x fettle-dev; rm -rf ~/Library/Application\ Support/Fettle\ Dev`.
- Don't test with `cargo run`. macOS would give Accessibility to the terminal, not to Fettle Dev.

### Accessibility

Fettle needs Accessibility to copy the selection and paste the result. `make dev` signs every build with a local self-signed identity, "Fettle Dev", so macOS keeps the grant across rebuilds. `scripts/dev-identity.sh` makes it on first use, in its own keychain at `~/Library/Keychains/fettle-dev.keychain-db`.

The user turns on Fettle Dev in System Settings > Privacy & Security > Accessibility once. If it isn't on, ask them. Don't try to turn it on yourself. If the grant stops working, `tccutil reset Accessibility dev.iyazerski.fettle.dev` clears it, and the next `make dev` asks again.

### Check a change end to end

1. Run `make dev` and check the log for errors.
2. With desktop automation, select text in an app like TextEdit and press ⌃⌥⇧E.
3. Check the Fettle Dev window, then quit it with `pkill -x fettle-dev`.

Debug builds are slower than releases. If something only goes wrong in a release, ask the user to try `make install`.
