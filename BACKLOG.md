# Backlog

## Refactoring

- [ ] **Refactor `PasskeyOp` enum to callback pattern.** Replace per-feature enum variants with a generic `PendingAssertion` struct holding a callback closure, to avoid adding a new variant for every Touch ID interaction. Currently each new async feature (unlock, new account, signing, etc.) requires a new `PasskeyOp` variant, a `poll_passkey_ops` match arm, and a `finish_*` method.
- [ ] **Stop exposing `qpv2-core::constants` as public.** The module was made `pub` so the GUI can access CKB code hash/hash type constants for balance queries. Instead, expose a helper (e.g. `lock_script_info(is_mainnet)`) in `qpv2_core::utilities` and revert to `mod constants`. This avoids leaking internal crypto constants like `SALT_LENGTH`, `ENC_SCRYPT`, and `PRF_HKDF_DOMAIN`.

## CI/CD

- [ ] **Fix CI for workspace restructure.** The lint job runs on `ubuntu-latest` but the workspace includes macOS-only crates (`qpv2-gui`, `passkey-prf`). Scope `cargo clippy`/`cargo fmt` to cross-platform crates, or move the lint job to a macOS runner.
- [ ] **Revert storage path before merging.** `db/mod.rs` changed data directory from `~/.quantum-purse/` to `~/Desktop/quantum-purse/`. Revert to `~/.quantum-purse/` before merging to `develop`.
- [ ] **Add GUI release workflow.** No CI job exists for building and signing `qpv2-gui`. Options: local-only release via `build-and-sign.sh` (attach to GitHub release manually), or CI release with Apple signing certificate and provisioning profile stored as GitHub Actions secrets.

## Architecture

- [ ] **Consider migrating GUI background I/O to tokio.** Balance fetching currently uses `std::thread` + `mpsc` channel. If the app grows to need more concurrent I/O (transaction broadcasting, node health polling, WebSocket subscriptions), a tokio runtime would provide structured concurrency and multiplexed I/O on fewer threads. Would require replacing `ureq` (blocking) with `reqwest` (async) in `node-manager`.
- [ ] **Add eframe persistence for GUI state.** Hook into eframe's `App::save()` to save/restore GUI state (selected node config, active tab, window position) across sessions. Called on shutdown and optionally at intervals when the `persistence` feature is enabled. Reference: `eframe::epi::App::save()` (`eframe-0.33.3/src/epi.rs:170`).

## Performance

- [ ] **Cache CKB addresses instead of recomputing every frame.** `lock_args_to_address` is called inside the `show_accounts_tab` render loop, re-encoding addresses on every repaint. Store computed addresses in a cache, recompute only on unlock, network toggle, or new account creation.

## Security

- [ ] **Implement re-validation before signing**
- [ ] **Ensure all dispatched calls are managed securely**
- [ ] **How much concurrency are being managed?**

## Chain / Sync

- [ ] **Reorg handling for tx history.** `tx_history.json` currently freezes records once their block is below the watermark. CKB reorgs (rare) would leave stale records in the store. Maintain a mutable "reorg window" of the last ~24 blocks: re-fetch on each tick, reconcile pending↔committed, remove records whose hash is no longer on chain. Below the window, finalize.
- [ ] **Cancellable tx-history sync thread.** The sync thread retries transient RPC failures forever. If the network is permanently down, the thread stays alive until app quit (harmless but inelegant). Add a cancel flag (e.g. an `Arc<AtomicBool>` reset when the user locks the wallet or changes node config) so retries terminate promptly.