//! Copies the brew-installed `pinentry-mac.app` next to the
//! built `qpv2-gui` so dev (`cargo run`) and release (`qpv2.app`)
//! both find it via the same binary-adjacent lookup that
//! `auth::pinentry_path()` performs at runtime.
//!
//! Source: brew installs the real binary as a Cocoa `.app` bundle at
//! `/opt/homebrew/Cellar/pinentry-mac/<version>/pinentry-mac.app`
//! (Apple Silicon) or under `/usr/local/Cellar/...` (Intel). The
//! file at `/opt/homebrew/bin/pinentry-mac` is a 111-byte shell
//! wrapper with a hardcoded Cellar path — bundling that wrapper
//! breaks for any machine without the same brew layout. We
//! resolve the symlink chain to the actual `.app` and copy the
//! whole directory tree.
//!
//! Failures are warnings, not errors. Without the .app the GUI
//! still builds; the password flow surfaces a runtime error
//! pointing at brew. The Touch ID flow remains usable either way.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if !cfg!(target_os = "macos") {
        return;
    }

    // Two layers of brew indirection:
    //   /opt/homebrew/bin/pinentry-mac  →  ../Cellar/pinentry-mac/<v>/bin/pinentry-mac (a script)
    //   The .app sits next to that bin/ dir at:
    //   /opt/homebrew/Cellar/pinentry-mac/<v>/pinentry-mac.app
    // We canonicalize the symlink to find the version dir, then walk up.
    let wrapper_candidates = [
        "/opt/homebrew/bin/pinentry-mac",
        "/usr/local/bin/pinentry-mac",
    ];
    let wrapper = match wrapper_candidates.iter().find(|p| Path::new(p).exists()) {
        Some(p) => PathBuf::from(p),
        None => {
            println!(
                "cargo:warning=pinentry-mac not found in /opt/homebrew/bin or \
                 /usr/local/bin. Run `brew install pinentry-mac` so the \
                 password flow works in dev and release builds."
            );
            return;
        }
    };

    let real = match std::fs::canonicalize(&wrapper) {
        Ok(p) => p,
        Err(e) => {
            println!(
                "cargo:warning=Could not canonicalize {}: {}",
                wrapper.display(),
                e
            );
            return;
        }
    };

    // `real` = .../Cellar/pinentry-mac/<version>/bin/pinentry-mac
    // We want    .../Cellar/pinentry-mac/<version>/pinentry-mac.app
    let version_dir = match real.parent().and_then(|p| p.parent()) {
        Some(p) => p.to_path_buf(),
        None => {
            println!(
                "cargo:warning=Unexpected pinentry-mac layout at {}",
                real.display()
            );
            return;
        }
    };
    let app_src = version_dir.join("pinentry-mac.app");
    if !app_src.exists() {
        println!(
            "cargo:warning=pinentry-mac.app not found at {} (expected sibling \
             of the brew wrapper). Reinstall via `brew reinstall pinentry-mac`.",
            app_src.display()
        );
        return;
    }

    // Cargo gives `OUT_DIR = target/<profile>/build/<crate-hash>/out` —
    // we walk up to the profile directory (target/debug or target/release)
    // where the `qpv2-gui` binary lands.
    let out_dir = match std::env::var_os("OUT_DIR") {
        Some(v) => PathBuf::from(v),
        None => return,
    };
    let profile_dir = match out_dir.ancestors().nth(3) {
        Some(d) => d.to_path_buf(),
        None => {
            println!(
                "cargo:warning=Could not derive profile dir from OUT_DIR; \
                 skipping pinentry-mac.app copy."
            );
            return;
        }
    };

    let app_dst = profile_dir.join("pinentry-mac.app");

    // Wipe the previous copy. brew installs read-only (0555); a
    // straight overwrite with std::fs::copy fails. `remove_dir_all`
    // handles both the directory and the read-only members.
    let _ = std::fs::remove_dir_all(&app_dst);

    if let Err(e) = copy_dir_recursive(&app_src, &app_dst) {
        println!(
            "cargo:warning=Failed to copy pinentry-mac.app to {}: {}",
            app_dst.display(),
            e
        );
    }
}

/// Recursive directory copy. Preserves contents but normalizes mode
/// so subsequent rebuilds can overwrite the destination cleanly.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_symlink() {
            // pinentry-mac.app has no symlinks, but be safe.
            let target = std::fs::read_link(&from)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &to)?;
            #[cfg(not(unix))]
            {
                let _ = target;
            }
        } else {
            std::fs::copy(&from, &to)?;
            // Make the copy writable so a future build can overwrite it.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perm = std::fs::metadata(&to)?.permissions();
                perm.set_mode(perm.mode() | 0o200);
                std::fs::set_permissions(&to, perm)?;
            }
        }
    }
    Ok(())
}
