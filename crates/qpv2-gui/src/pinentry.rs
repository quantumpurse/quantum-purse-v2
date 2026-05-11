//! Out-of-process password input via the `pinentry` crate.
//!
//! Replaces the in-process egui modal: keystrokes never enter this
//! binary's address space. The crate spawns a child `pinentry-mac`
//! process which owns the Cocoa `NSSecureTextField` (mlock'd buffer +
//! `EnableSecureEventInput()`); we receive the password as a single
//! kernel-pipe read at submit time, copy it once into a
//! zeroize-on-drop `SecureString`, and drop it the moment the vault
//! op returns.
//!
//! Lookup is binary-adjacent only — `current_exe().parent()/pinentry-mac`.
//! For release this resolves inside `qpv2.app/Contents/MacOS/`; for
//! `cargo run` the build script (`build.rs`) copies the from-source
//! build (`vendor/pinentry-build/`) next to `target/{debug,release}/qpv2-gui`.
//! There is no `$PATH` fallback by design, so dev and release exercise
//! the same resolution path.
//!
//! `interact()` blocks the calling thread for the duration of the
//! dialog. Called from the egui update loop — frames freeze while the
//! modal is up. Acceptable: the user is interacting with the dialog,
//! not the wallet UI, and the GUI's per-frame async pollers don't
//! drive any time-critical work that would suffer from a few seconds
//! of pause.

use pinentry::{Error as PinentryError, PassphraseInput};
use qpv2_core::SecureString;
use secrecy::ExposeSecret;
use std::path::PathBuf;

/// Resolves to the inner Mach-O of the bundled
/// `<dir-of-current-exe>/pinentry-mac.app`.
///
/// `pinentry-mac` is a Cocoa .app — the binary needs its sibling
/// `Resources/` (nibs, etc.) to draw the dialog, so we bundle the
/// entire `.app` rather than the inner binary alone.
fn pinentry_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("current_exe() failed: {}", e))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current_exe() has no parent".to_string())?;
    let path = dir
        .join("pinentry-mac.app")
        .join("Contents")
        .join("MacOS")
        .join("pinentry-mac");
    if !path.exists() {
        let msg = format!(
            "pinentry-mac.app not found at {}. \
             The app bundle is incomplete.",
            path.display()
        );
        // .app bundles launched via `open` swallow stdout/stderr from the
        // terminal but route them to Console.app — `eprintln!` keeps a
        // breadcrumb visible there even when the in-app banner clears.
        eprintln!("auth: {}", msg);
        return Err(msg);
    }
    Ok(path)
}

/// Maps pinentry errors to user-facing strings.
fn map_err(e: PinentryError) -> String {
    match e {
        PinentryError::Cancelled => "Cancelled.".to_string(),
        PinentryError::Timeout => "Password entry timed out.".to_string(),
        PinentryError::Io(e) => format!("Password dialog I/O error: {}", e),
        PinentryError::Encoding(_) => {
            "Password is not valid UTF-8.".to_string()
        }
        PinentryError::Gpg(g) => format!("Password dialog error: {}", g),
    }
}

/// Open a single-field password dialog. `description` appears above
/// the field, `prompt` immediately to the left of it. The returned
/// `SecureString` is zeroize-on-drop; the caller is responsible for
/// not cloning it into unmanaged buffers.
pub(crate) fn prompt_password(
    description: &str,
    prompt: &str,
) -> Result<SecureString, String> {
    let path = pinentry_path()?;
    let mut input = PassphraseInput::with_binary(&path)
        .ok_or_else(|| format!("pinentry-mac not executable at {}", path.display()))?;
    let secret = input
        .with_title("Quantum Purse")
        .with_description(description)
        .with_prompt(prompt)
        .with_ok("Authorize")
        .with_cancel("Cancel")
        .interact()
        .map_err(map_err)?;
    Ok(SecureString::from_string(secret.expose_secret().to_string()))
}

/// Open a password dialog with a confirmation field. pinentry's
/// `SETREPEAT` makes the binary itself enforce match — we receive a
/// single `SecretString` only after both fields agree. `mismatch_error`
/// is what the dialog shows in-place when they don't.
pub(crate) fn prompt_password_with_confirmation(
    description: &str,
    prompt: &str,
    confirm_prompt: &str,
    mismatch_error: &str,
) -> Result<SecureString, String> {
    let path = pinentry_path()?;
    let mut input = PassphraseInput::with_binary(&path)
        .ok_or_else(|| format!("pinentry-mac not executable at {}", path.display()))?;
    let secret = input
        .with_title("Quantum Purse")
        .with_description(description)
        .with_prompt(prompt)
        .with_confirmation(confirm_prompt, mismatch_error)
        .with_ok("Create")
        .with_cancel("Cancel")
        .interact()
        .map_err(map_err)?;
    Ok(SecureString::from_string(secret.expose_secret().to_string()))
}
