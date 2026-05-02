//! Stages `pinentry-mac.app` plus its three brew-supplied dylibs
//! (`libassuan`, `libgpg-error`, `libintl`) into
//! `target/{debug,release}/pinentry-mac.app`, with the dylibs'
//! install names rewritten to `@executable_path/../Frameworks/...`
//! so the structure matches what the signed release bundle needs.
//!
//! Why this lives here:
//! - **Cross-platform-ready.** When Windows / Linux ship later, the
//!   per-OS `cfg` arms add their own equivalent staging (DLLs next
//!   to a `.exe`, GTK libs in an AppImage tree, etc.). The release
//!   script per platform consumes the staged tree without re-doing
//!   discovery.
//! - **Dev parity.** `cargo run` finds the `.app` via the same
//!   `current_exe().parent()/pinentry-mac.app/...` lookup that
//!   `auth::pinentry_path()` uses inside the signed `qpv2.app`.
//! - **Single source of truth.** Brew layout, dylib paths, and
//!   install-name rewrites live in one place. The release script
//!   (`build-and-sign.sh`) just `cp -R`s the staged tree, rewrites
//!   the *binary's* load commands (which would invalidate dev's
//!   signature if done here), and codesigns under Developer ID.
//!
//! Source layout reminder:
//!   /opt/homebrew/bin/pinentry-mac  →  ../Cellar/pinentry-mac/<v>/bin/pinentry-mac (a 111-byte shell wrapper)
//!   /opt/homebrew/Cellar/pinentry-mac/<v>/pinentry-mac.app                          (the real Cocoa .app)
//!   /opt/homebrew/opt/{libassuan,libgpg-error,gettext}/lib/lib*.dylib              (the link-time deps)
//!
//! Failures are warnings, not errors. Without the staged tree the
//! GUI still builds; the password flow surfaces a runtime error
//! pointing at brew. The Touch ID flow remains usable either way.

use std::path::{Path, PathBuf};
use std::process::Command;

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
        return;
    }

    if let Err(e) = stage_dylibs(&app_dst) {
        println!("cargo:warning=Failed to stage pinentry dylibs: {}", e);
    }
}

/// Copies `pinentry-mac`'s three brew dylibs into the staged
/// `.app/Contents/Frameworks/` and rewrites their install names so
/// the dependency chain points at `@executable_path/../Frameworks/`
/// instead of `/opt/homebrew/opt/...`.
///
/// Dev never loads these copies (the staged binary's `LC_LOAD_DYLIB`
/// still points at brew's original paths and brew's signature stays
/// intact), so the install-name rewrites here are inert in dev — but
/// they're exactly what the release script needs after it `cp -R`s
/// the staged tree into `qpv2.app/Contents/MacOS/pinentry-mac.app`.
/// The script then rewrites the *binary's* load commands and
/// codesigns the chain under our Developer ID.
fn stage_dylibs(app_dst: &Path) -> Result<(), String> {
    let frameworks = app_dst.join("Contents").join("Frameworks");
    std::fs::create_dir_all(&frameworks)
        .map_err(|e| format!("create Frameworks dir: {}", e))?;

    // (source path, file name) — the install-name rewrites below
    // use the file names; sources are brew's canonical opt/ symlinks.
    let dylibs = [
        ("/opt/homebrew/opt/libassuan/lib/libassuan.9.dylib", "libassuan.9.dylib"),
        ("/opt/homebrew/opt/libgpg-error/lib/libgpg-error.0.dylib", "libgpg-error.0.dylib"),
        ("/opt/homebrew/opt/gettext/lib/libintl.8.dylib", "libintl.8.dylib"),
    ];
    for (src, name) in &dylibs {
        let src_path = Path::new(src);
        if !src_path.exists() {
            return Err(format!(
                "required dylib missing: {}. Reinstall: brew reinstall \
                 libassuan libgpg-error gettext",
                src
            ));
        }
        let dst = frameworks.join(name);
        let _ = std::fs::remove_file(&dst);
        std::fs::copy(src_path, &dst)
            .map_err(|e| format!("copy {}: {}", src, e))?;
        // brew installs 0444; install_name_tool needs write access.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&dst)
                .map_err(|e| format!("stat {}: {}", dst.display(), e))?
                .permissions();
            perm.set_mode(perm.mode() | 0o200);
            std::fs::set_permissions(&dst, perm)
                .map_err(|e| format!("chmod {}: {}", dst.display(), e))?;
        }
    }

    let assuan = frameworks.join("libassuan.9.dylib");
    let gpgerr = frameworks.join("libgpg-error.0.dylib");
    let intl = frameworks.join("libintl.8.dylib");

    // Rewrite each dylib's LC_ID so dyld lookups resolve to the
    // bundled copy rather than the original brew path.
    install_name_tool(&["-id", "@executable_path/../Frameworks/libassuan.9.dylib", &assuan.to_string_lossy()])?;
    install_name_tool(&["-id", "@executable_path/../Frameworks/libgpg-error.0.dylib", &gpgerr.to_string_lossy()])?;
    install_name_tool(&["-id", "@executable_path/../Frameworks/libintl.8.dylib", &intl.to_string_lossy()])?;

    // Rewrite inter-dylib LC_LOAD_DYLIB references through the chain.
    // libassuan → libgpg-error
    install_name_tool(&[
        "-change",
        "/opt/homebrew/opt/libgpg-error/lib/libgpg-error.0.dylib",
        "@executable_path/../Frameworks/libgpg-error.0.dylib",
        &assuan.to_string_lossy(),
    ])?;
    // libgpg-error → libintl
    install_name_tool(&[
        "-change",
        "/opt/homebrew/opt/gettext/lib/libintl.8.dylib",
        "@executable_path/../Frameworks/libintl.8.dylib",
        &gpgerr.to_string_lossy(),
    ])?;

    // The pinentry-mac binary's LC_LOAD_DYLIB rewrite stays in
    // build-and-sign.sh: doing it here would invalidate brew's
    // signature on the dev binary, which dev still relies on to
    // launch under hardened runtime.

    Ok(())
}

fn install_name_tool(args: &[&str]) -> Result<(), String> {
    let out = Command::new("install_name_tool")
        .args(args)
        .output()
        .map_err(|e| format!("install_name_tool exec: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "install_name_tool {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
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
