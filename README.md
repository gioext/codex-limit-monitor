# Codex Limit Monitor

Codex Limit Monitor is an unofficial macOS menu bar app that keeps your Codex usage limits visible at a glance.

It shows:

- Remaining weekly usage as a progress bar
- The next weekly reset date and time
- The number of available limit resets
- The expiration date of each available reset
- Automatic refresh every five minutes and manual refresh from the tray menu

## Requirements

- macOS 11 or later
- [Node.js](https://nodejs.org/) and npm
- [Rust](https://www.rust-lang.org/tools/install) and Cargo
- Codex CLI installed and signed in with a ChatGPT account
- Xcode Command Line Tools (`xcode-select --install`)

## Development

Install the dependencies and start the Tauri development app:

```bash
npm install
npm run check
npm run tauri dev
```

To work on the interface in a browser with preview data:

```bash
npm run dev
```

Run the frontend build and Rust tests before packaging:

```bash
npm run build
(cd src-tauri && cargo test)
```

## Installation from source

Build the release app and copy it to the Applications folder:

```bash
npm install
npm run tauri build
ditto "src-tauri/target/release/bundle/macos/Codex Limit Monitor.app" \
  "/Applications/Codex Limit Monitor.app"
open "/Applications/Codex Limit Monitor.app"
```

Quit an existing copy of Codex Limit Monitor before replacing it.

## Data access and privacy

The app requests usage data from the local `codex app-server` process through the read-only `account/rateLimits/read` method. This does not start a model turn or consume Codex usage.

If the app-server response reports available reset credits without expiration details, the app reads the existing Codex login file at `~/.codex/auth.json` and requests the missing details from the fixed ChatGPT endpoint `https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`. The access token and account ID are kept in memory only and are not logged, persisted, or sent to any other host.

The app contains no analytics or telemetry.

## Disclaimer

This is an unofficial community project and is not affiliated with or endorsed by OpenAI. It relies on Codex app-server and ChatGPT response formats that may change without notice.
