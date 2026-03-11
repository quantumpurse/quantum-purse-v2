# QPV2

Quantum Purse version 2 built entirely in Rust. Secure and Performant. There are 2 UI options: CLI and GUI (egui).

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
qpv2 init --variant <VARIANT> # example: qpv2 init --variant Sha2256S

# ImportMnemonic an existing wallet
qpv2 mnemonic import

# ExportMnemonic seed phrase
qpv2 mnemonic export

# Generate a new account
qpv2 account new

# List all accounts
qpv2 account list

# Sign and generate a raw sphincs+ signature for any message
qpv2 sign --identifier <IDENTIFIER> --message <MESSAGE>

# Sign a message - designed for CKB transaction
qpv2 ckb sign --lock-args <LOCK_ARGS> --message <MESSAGE>

# Recover accounts
qpv2 ckb recover --count <COUNT>

# Generate account batch for discovery
qpv2 ckb try-gen-batch --start <START> --count <COUNT> # example: qpv2 try-gen-batch --start 0 --count 10

# Get CKB transaction message hash
qpv2 ckb get-ckb-tx-message --tx-file <TX_FILE>

# Clear all vault data
qpv2 clear

# Show help
qpv2 --help
```

### Use GUI
```shell
# launch the dev gui
./launch.sh

# launch the prod gui
./launch.sh --release
```

### Data Storage

Wallet data is stored in `~/.quantum-purse/`:
- `master_seed.json` - Encrypted master seed
- `accounts.json` - Encrypted account private keys

### Supported SPHINCS+ Variants

- Sha2128F, Sha2128S
- Sha2192F, Sha2192S
- Sha2256F, Sha2256S
- Shake128F, Shake128S
- Shake192F, Shake192S
- Shake256F, Shake256S
