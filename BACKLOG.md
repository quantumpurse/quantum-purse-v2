# Backlog

## Refactoring

- [ ] **Refactor `PendingOp` enum to callback pattern.** Replace per-feature enum variants with a generic `PendingAssertion` struct holding a callback closure, to avoid adding a new variant for every Touch ID interaction. Currently each new async feature (unlock, new account, signing, etc.) requires a new `PendingOp` variant, a `poll_pending` match arm, and a `finish_*` method.
- [ ] **Stop exposing `key-vault-core::constants` as public.** The module was made `pub` so the GUI can access CKB code hash/hash type constants for balance queries. Instead, expose a helper (e.g. `lock_script_info(is_mainnet)`) in `key_vault_core::utilities` and revert to `mod constants`. This avoids leaking internal crypto constants like `SALT_LENGTH`, `ENC_SCRYPT`, and `PRF_HKDF_DOMAIN`.

## CI/CD

- [ ] **Fix CI for workspace restructure.** The lint job runs on `ubuntu-latest` but the workspace includes macOS-only crates (`key-vault-gui`, `passkey-prf`). Scope `cargo clippy`/`cargo fmt` to cross-platform crates, or move the lint job to a macOS runner.
- [ ] **Revert storage path before merging.** `db/mod.rs` changed data directory from `~/.quantum-purse/` to `~/Desktop/quantum-purse/`. Revert to `~/.quantum-purse/` before merging to `develop`.
- [ ] **Add GUI release workflow.** No CI job exists for building and signing `qpkv-gui`. Options: local-only release via `build-and-sign.sh` (attach to GitHub release manually), or CI release with Apple signing certificate and provisioning profile stored as GitHub Actions secrets.

## Architecture

- [ ] **Consider migrating GUI background I/O to tokio.** Balance fetching currently uses `std::thread` + `mpsc` channel. If the app grows to need more concurrent I/O (transaction broadcasting, node health polling, WebSocket subscriptions), a tokio runtime would provide structured concurrency and multiplexed I/O on fewer threads. Would require replacing `ureq` (blocking) with `reqwest` (async) in `node-manager`.

## Performance

- [ ] **Cache CKB addresses instead of recomputing every frame.** `lock_args_to_address` is called inside the `show_accounts_tab` render loop, re-encoding addresses on every repaint. Store computed addresses in a cache, recompute only on unlock, network toggle, or new account creation.
