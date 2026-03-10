# Backlog

## Refactoring

- [ ] **Refactor `PendingOp` enum to callback pattern.** Replace per-feature enum variants with a generic `PendingAssertion` struct holding a callback closure, to avoid adding a new variant for every Touch ID interaction. Currently each new async feature (unlock, new account, signing, etc.) requires a new `PendingOp` variant, a `poll_pending` match arm, and a `finish_*` method.

## CI/CD

- [ ] **Fix CI for workspace restructure.** The lint job runs on `ubuntu-latest` but the workspace includes macOS-only crates (`key-vault-gui`, `passkey-prf`). Scope `cargo clippy`/`cargo fmt` to cross-platform crates, or move the lint job to a macOS runner.
- [ ] **Revert storage path before merging.** `db/mod.rs` changed data directory from `~/.quantum-purse/` to `~/Desktop/quantum-purse/`. Revert to `~/.quantum-purse/` before merging to `develop`.
- [ ] **Add GUI release workflow.** No CI job exists for building and signing `qpkv-gui`. Options: local-only release via `build-and-sign.sh` (attach to GitHub release manually), or CI release with Apple signing certificate and provisioning profile stored as GitHub Actions secrets.
