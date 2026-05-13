# QPV2

Quantum Purse version 2 built entirely in Rust. Secure and Performant. There are 2 UI options: CLI and GUI (egui).

Developed in collaboration with Claude Opus (4.5 / 4.6 / 4.7): developer-led architecture, abstraction boundaries, and design decisions; Claude-authored implementation under per-step review.

###### <u>Feature list</u>:

| Feature               | Details              |
|-----------------------|----------------------|
| **Signature type**    | SPHINCS+             |
| **Store model**       | File-based (JSON)    |
| **Mnemonic standard** | Custom BIP39 English |
| **Local encryption**  | AES256               |
| **Key derivation**    | HKDF-SHA256          |
| **Authentication**    | Password / Touch ID (macOS) |
| **Password hashing**  | Scrypt               |
| **Platform**          | macOS, Windows, Linux |

### Custom BIP39
BIP39 is chosen as the mnemonic backup format due to its user-friendliness and quantum resistance.

SPHINCS+ offers 12 parameter sets, grouped by three security parameters: 128-bit, 192-bit, and 256-bit. These require seeds of 48 bytes, 72 bytes, and 96 bytes respectively used across key generation and signing. As BIP39 supports max 32 bytes so this library introduces a custom(combined) BIP39 mnemonic backup format for each security parameter of SPHINCS+ as below:

|    SPHINCS+ security parameter      |  BIP39 entropy level  |   Word count    |
|-------------------------------------|-----------------------|-----------------|
|    128 bit ~ 48 bytes ~ 3*16 bytes  |       3*16 bytes      | 3*12 = 36 words |
|    192 bit ~ 72 bytes ~ 3*24 bytes  |       3*24 bytes      | 3*18 = 54 words |
|    256 bit ~ 96 bytes ~ 3*32 bytes  |       3*32 bytes      | 3*24 = 72 words |

###### For example:
- SHA2-256s will require users to back up 72 words of mnemonic phrase.
- SHAKE-192s will require users to back up 54 words of mnemonic phrase.
- SHA2-128f will require users to back up 36 words of mnemonic phrase.

### Key Derivation Function

From the single master seed, quantum-purse-v2 can derive many child keys using Key Derivation Function(KDF). Pure Hash-based KDF is the top choice for this project. Although using [BIP32](https://en.bitcoin.it/wiki/BIP_0032) carefully (with only hardened key derivation and never generate ECDSA public keys) can satisfy however the benefits of the tricky usage at this point(2025) is unclear. Thus, a fresh start with HKDF seems better because it's simpler - meaning the implementation will be easier to audit.

###### Key Tree:
```
master_seed
   ├─ index 0 → sphincs+ key 1
   ├─ index 1 → sphincs+ key 2
   ├─ index 2 → sphincs+ key 3
   └─ ...
```

###### Derivation Flow:
```
master_seed
     │
     ▼
(seed_part1, seed_part2, seed_part3)
     │
     ├─ HKDF("ckb/quantum-purse/sphincs-plus/", index)
     │
     ▼
(sk_seed, sk_prf, pk_seed)
     │
     ├─ sphincs+_key_gen()
     │
     ▼
(sphincs+ public_key, sphincs+ private_key)
```

### Dependencies

- Rust & Cargo (1.70+)

### Developer Build Toolchain

Local GUI development and build scripts (`build.sh`, `launch.sh`,
`crates/qpv2-gui/scripts/`) are designed for macOS only. Linux and
Windows GUI builds are handled by the CI/CD workflow.

The GUI's password dialog (`pinentry`) is built from source via
`vendor/build-pinentry.sh`. Developers on macOS need:

| Tool | Install | Purpose |
|------|---------|---------|
| `automake` | `brew install automake` | Generates Makefiles for C deps |
| `gettext` | `brew install gettext` | Provides m4 macros for autotools |
| Xcode CLI tools | `xcode-select --install` | Obj-C compiler + ibtool for nib files |

### Build CLI
```shell
# Build dev
cargo build -p qpv2-cli

# Build release
cargo build -p qpv2-cli --release

# Sign for Touch ID (macOS only, required after every build)
codesign -s "Developer ID Application: Pham Tung (KPSL53752R)" \
  --entitlements crates/qpv2-gui/entitlements.plist \
  --force target/release/qpv2-cli

# Run release binary
./target/release/qpv2-cli --help

# Run tests
cargo test -p qpv2-cli
```

Signing is only needed for Touch ID (`--keychain`) support. Password-only wallets work without signing.

### Build GUI (macOS)
```shell
# Build dev binary
./build.sh

# Build prod binary
./build.sh --release

# Run tests
cargo test --workspace
```

### Use CLI

The CLI provides the following commands:

```shell
# Initialize a new wallet (password)
qpv2-cli init --variant <VARIANT>

# Initialize a new wallet (Touch ID, macOS only)
qpv2-cli init --variant <VARIANT> --keychain

# Import mnemonic (password)
qpv2-cli mnemonic import --variant <VARIANT>

# Import mnemonic (Touch ID, macOS only)
qpv2-cli mnemonic import --variant <VARIANT> --keychain

# Export mnemonic seed phrase
qpv2-cli mnemonic export

# Generate a new account
qpv2-cli account new

# List all accounts
qpv2-cli account list

# Recover accounts
qpv2-cli account recover --count <COUNT>

# Generate account batch for discovery
qpv2-cli account try-gen-batch --start <START> --count <COUNT>

# Sign a raw SPHINCS+ message
qpv2-cli sign --identifier <IDENTIFIER> --message <MESSAGE>

# Sign a CKB transaction message
qpv2-cli ckb sign --lock-args <LOCK_ARGS> --message <MESSAGE>

# Get CKB transaction message hash
qpv2-cli ckb get-tx-message --tx-file <TX_FILE>

# Display wallet information
qpv2-cli info

# Clear all vault data
qpv2-cli clear

# Show help
qpv2-cli --help
```

Commands that require authentication (export, new account, sign, etc.) auto-detect the wallet's auth method. Password wallets prompt for a password; Touch ID wallets trigger the system biometric dialog.

### Use GUI
```shell
# launch the dev gui
./launch.sh

# launch the prod gui
./launch.sh --release
```

### Node Backends

The GUI lets the user pick how the wallet talks to the CKB chain. Each
backend is selectable from the Node Manager tab and persists across
sessions.

| Backend       | What it is                                         | When to use |
|---------------|----------------------------------------------------|-------------|
| **Public RPC**| Remote JSON-RPC endpoint (default).                | Quick start, no local resources. |
| **Light Client** | Local `ckb-light-client` child process; header-only sync, per-script cell index. | Privacy-preserving; modest disk + bandwidth. |
| **Full Node** | Local `ckb` child process; full chain verification, full indexer. | Maximum sovereignty; ~100 GB disk and multi-day sync. |

Both local backends are bundled inside the signed `qpv2.app`
(`Contents/MacOS/{ckb-light-client,ckb}`) and spawned/stopped by the GUI
automatically. Per-network data dirs live under `~/Library/Application
Support/quantum-purse/node/`.

### Password Input

Password entry in QPV2 happens **outside the wallet's own process** —
through a dedicated, OS-native dialog spawned as a child process. The
wallet binary itself never sees a keystroke during typing; it only
reads the final password from a kernel pipe at the moment the user
submits, copies it once into a zeroize-on-drop `SecureString`, and
drops it the moment the vault op (sign / decrypt / new account)
returns.

#### Why not just use an egui text field?

A straightforward `egui::TextEdit::singleline(&mut String).password(true)`
would have the password live inside the wallet's own heap **for the
entire typing duration** (potentially many seconds), with `String`
reallocations during keystrokes leaving orphan plaintext fragments in
freed memory that no application code can zero. Out-of-process entry
sidesteps both: typing happens in the dialog program's address space,
and our process gets the bytes as a single small read at the end —
sub-millisecond exposure window before the bytes enter `SecureString`.

#### How: the `pinentry` crate

QPV2 uses the [`pinentry`](https://docs.rs/pinentry) Rust crate, which
wraps the GnuPG-project `pinentry-*` family of dialog binaries via the
[Assuan protocol](https://www.gnupg.org/documentation/manuals/assuan/)
(line-based text over stdin/stdout pipes). The same Rust call site
works on every supported OS — only the bundled binary differs:

| OS      | Bundled binary       | UI rendering |
|---------|----------------------|----------------------------------------------|
| macOS   | `pinentry-mac`       | Native Cocoa window with `NSSecureTextField` (mlock'd buffer + `EnableSecureEventInput()` to block other apps from tapping the keystrokes) |
| Windows | `pinentry-w64.exe`   | Native Win32 dialog (with `SecureZeroMemory` backing) |
| Linux   | `pinentry-gtk-2`     | GTK 2 dialog with secure-entry mode (mlock + clipboard blocking); `pinentry-curses` ncurses fallback for headless |

The binaries ship inside the application bundle / installer — end
users install nothing. Each platform's dialog inherits that OS's
**purpose-built secure-input infrastructure**: NSSecureTextField on
macOS prevents accessibility-API observers, screen recorders, and IME
services from seeing the field's content; the equivalent Win32 and
GTK widgets do similar.

#### What pinentry does *not* protect against

- An OS-level keylogger above the dialog still sees keystrokes — same
  as it would for any password input on the system. Hardware wallets
  / Touch ID / Windows Hello / Secure Enclave are the only categorical
  defenses.
- A compromised bundled `pinentry-*` binary is game-over (same threat
  as a compromised wallet binary). Both are signed/notarized at build
  time.

The "PIN" in pinentry is historical naming — the binaries handle full
passphrases (any length, full Unicode), not just numeric PINs.

### Data Storage

Wallet state lives in the platform-standard application data dir:

- macOS:   `~/Library/Application Support/quantum-purse/`
- Linux:   `~/.local/share/quantum-purse/`
- Windows: `%APPDATA%\quantum-purse\`

Files:

- `master_seed.json` — encrypted master seed
- `accounts.json` — derived SPHINCS+ accounts
- `wallet_info.json` — variant + auth method
- `tx_history_<network>.json` — per-network tx-history cache
- `node/` — node-manager state (config + per-backend chain dirs)

### Supported SPHINCS+ Variants

- Sha2128F, Sha2128S
- Sha2192F, Sha2192S
- Sha2256F, Sha2256S
- Shake128F, Shake128S
- Shake192F, Shake192S
- Shake256F, Shake256S
