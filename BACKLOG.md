# Backlog

## Refactoring

- [ ] **Stop exposing `qpv2-core::constants` as public.** The module was made `pub` so the GUI can access CKB code hash/hash type constants for balance queries. Instead, expose a helper (e.g. `lock_script_info(is_mainnet)`) in `qpv2_core::utilities` and revert to `mod constants`. This avoids leaking internal crypto constants like `SALT_LENGTH`, `ENC_SCRYPT`, and `VAULT_ENC_KEY_HKDF_INFO`.

## CI/CD

- [ ] **Add GUI release workflow.** No CI job exists for building and signing `qpv2-gui`. Options: local-only release via `build-and-sign.sh` (attach to GitHub release manually), or CI release with Apple signing certificate and provisioning profile stored as GitHub Actions secrets.

## Architecture

- [ ] **Consider migrating GUI background I/O to tokio.** Balance fetching currently uses `std::thread` + `mpsc` channel. If the app grows to need more concurrent I/O (transaction broadcasting, node health polling, WebSocket subscriptions), a tokio runtime would provide structured concurrency and multiplexed I/O on fewer threads. Would require replacing `ureq` (blocking) with `reqwest` (async) in `node-manager`.
- [ ] **Add eframe persistence for GUI state.** Hook into eframe's `App::save()` to save/restore GUI state (selected node config, active tab, window position) across sessions. Called on shutdown and optionally at intervals when the `persistence` feature is enabled. Reference: `eframe::epi::App::save()` (`eframe-0.33.3/src/epi.rs:170`).

## Performance

- [ ] **Cache CKB addresses instead of recomputing every frame.** `lock_args_to_address` is called inside the `show_accounts_tab` render loop, re-encoding addresses on every repaint. Store computed addresses in a cache, recompute only on unlock, network toggle, or new account creation.

## Developer Pitfalls

## FIDO2

- [ ] **Support built-in user verification (UV) for FIDO2 devices.** Devices with on-board biometrics (YubiKey Bio fingerprint) or on-device PIN entry (keypads/buttons) handle user verification internally — no host-side PIN is needed. Detect device UV capability at registration/assertion time and skip the PIN prompt when the device supports internal UV. Currently only the clientPin path is implemented (PIN entered on host, sent encrypted to device).

## Security

- [ ] **Implement re-validation before signing**
- [ ] **Ensure all dispatched calls are managed securely**
- [ ] **How much concurrency are being managed?**
- [ ] **Patch `pinentry` crate's `BufReader` so its scratch buffer zeroizes on drop.** In `pinentry-0.8.0/src/assuan.rs`, `Connection::input` is a `BufReader<ChildStdout>` (line 50) whose internal `Vec<u8>` receives the password bytes via `read_line` (line 142). The crate explicitly zeroizes every other plaintext copy (the `line` String, the `DataLine` `SecretString`, the percent-decoded `Cow`, the concat buffer), but `BufReader` has no zeroizing `Drop`, and `Connection`'s `Drop` impl (lines 190–205) doesn't reach in to scrub it. Net: one freed-but-not-zeroed page per password prompt — readable from freed-memory snapshots until the allocator reuses it. Fix paths: (1) upstream PR to `str4d/pinentry-rs` adding a zeroizing reader newtype around `BufReader` (preferred — benefits every consumer), or (2) fork the crate into `vendor/` and apply the patch with a path dep. Today's leak is ~1 fragment per prompt vs egui's ~5+, so accepted; revisit if we move to higher-frequency password prompts.

## Chain / Sync

- [ ] **Reorg handling for tx history.** `tx_history.json` currently freezes records once their block is below the watermark. CKB reorgs (rare) would leave stale records in the store. Maintain a mutable "reorg window" of the last ~24 blocks: re-fetch on each tick, reconcile pending↔committed, remove records whose hash is no longer on chain. Below the window, finalize.
- [ ] **Cancellable tx-history sync thread.** The sync thread retries transient RPC failures forever. If the network is permanently down, the thread stays alive until app quit (harmless but inelegant). Add a cancel flag (e.g. an `Arc<AtomicBool>` reset when the user locks the wallet or changes node config) so retries terminate promptly.