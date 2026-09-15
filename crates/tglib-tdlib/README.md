# tglib-tdlib

TDLib adapter for tglib’s `TelegramClient` trait.

- Default / CI: stub only. No `tdjson`.
- Live: `--features native` links Homebrew or system `tdjson`.

This crate never exports TDLib `@type` names. Mapping stays inside the adapter.

`api_id` / `api_hash` identify **your application**, not a user session.

## Native build (macOS)

Homebrew **stable** `tdlib` may be too old for login (`UPDATE_APP_TO_LOGIN`). Prefer HEAD / a current build:

```bash
brew uninstall tdlib
brew install --HEAD tdlib
pkg-config --libs tdjson
```

Or build from source (https://tdlib.github.io/td/build.html) and set:

```bash
export TDLIB_DIR=/path/to/tdlib/prefix
export TDLIB_LIB_DIR=$TDLIB_DIR/lib
```

## Credentials

1. Register an application at https://my.telegram.org
2. Put `api_id` / `api_hash` in `telegram.toml` (see repo `telegram.toml.example`)
3. Or set `TELEGRAM_API_ID` / `TELEGRAM_API_HASH`
4. Optional embed: `TGLIB_EMBEDDED_TELEGRAM_API_ID` / `_HASH`

```bash
cargo test -p tglib-tdlib --features native -- --nocapture
```
