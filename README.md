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
| **Authentication**    | Password             |
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

### Build
```shell
# Build dev binary
./build.sh

# Build prod binary
./build.sh --release

# Run tests
cargo test
```

### Use CLI

The CLI provides the following commands:

```shell
# Initialize a new wallet
qpv2-cli init --variant <VARIANT> # example: qpv2-cli init --variant Sha2256S

# ImportMnemonic an existing wallet
qpv2-cli mnemonic import

# ExportMnemonic seed phrase
qpv2-cli mnemonic export

# Generate a new account
qpv2-cli account new

# List all accounts
qpv2-cli account list

# Sign and generate a raw sphincs+ signature for any message
qpv2-cli sign --identifier <IDENTIFIER> --message <MESSAGE>

# Sign a message - designed for CKB transaction
qpv2-cli ckb sign --lock-args <LOCK_ARGS> --message <MESSAGE>

# Recover accounts
qpv2-cli ckb recover --count <COUNT>

# Generate account batch for discovery
qpv2-cli ckb try-gen-batch --start <START> --count <COUNT> # example: qpv2-cli try-gen-batch --start 0 --count 10

# Get CKB transaction message hash
qpv2-cli ckb get-ckb-tx-message --tx-file <TX_FILE>

# Clear all vault data
qpv2-cli clear

# Show help
qpv2-cli --help
```

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
