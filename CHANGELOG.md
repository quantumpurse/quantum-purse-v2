# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-05
**Note**: This is the initial release of the qpv2-cli, a fork from https://github.com/quantumpurse/quantum-purse-key-vault that's tuned to be a sphincs+ key management cli with CKB address resolution integration.

### Added
- SPHINCS+ signer for quantum-resistant signatures
- CKB blockchain address resolution integration
- HKDF-SHA256 for child key derivation (simpler and more auditable than BIP32)
- On-the-fly private key derivation (no cached private keys)
- File-based storage with AES-256-GCM encryption
- Custom BIP39 implementation supporting 36/54/72 word mnemonics
- Password authentication with Scrypt (log_n=17, r=8, p=1)
- Secure memory handling with automatic zeroization
- Support for all 12 SPHINCS+ parameter sets

### Security Features
- Minimum 20-character password requirement
- Scrypt parameters tuned for NIST category 1 quantum resistance
- Master seed only storage (private keys never cached)
- Automatic memory zeroization for sensitive data