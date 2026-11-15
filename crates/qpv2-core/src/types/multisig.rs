//! What a multisig is: its signers, its rules, the account saved for it, and
//! the lock and witness built from them. A single-sig account is the 1-of-1
//! case.

use super::SpxVariant;
use crate::constants::{
    CKB_MAINNET_CODE_HASH, CKB_MAINNET_HASH_TYPE, CKB_TESTNET_CODE_HASH, CKB_TESTNET_HASH_TYPE,
    MULTISIG_RESERVED_FIELD_VALUE,
};
use ckb_types::{bytes::Bytes, packed::Script, prelude::*, H256};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A signer in a multisig group: their place in it, SPHINCS+ variant and
/// public key.
///
/// The fields are private so that a signer from outside this crate can only
/// come from [`Signer::new`], which checks the key's length. Saved accounts
/// and signing requests are read back through the same check.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(try_from = "ReprSigner")]
pub struct Signer {
    /// This signer's place in its multisig, counting from 0. The signers at
    /// places below `required_first_n` must always sign.
    index: u8,
    variant: SpxVariant,
    #[serde(with = "hex::serde")]
    pubkey: Vec<u8>,
}

/// A `Signer` as written in JSON. Reading one goes through [`Signer::new`],
/// so a saved key of the wrong length is refused like a typed one.
#[derive(Deserialize)]
struct ReprSigner {
    index: u8,
    variant: SpxVariant,
    #[serde(with = "hex::serde")]
    pubkey: Vec<u8>,
}

impl TryFrom<ReprSigner> for Signer {
    type Error = String;

    fn try_from(repr: ReprSigner) -> Result<Self, String> {
        Signer::new(repr.index, repr.variant, repr.pubkey)
    }
}

impl Signer {
    /// A signer at `index` in its multisig, with its parameter set and public
    /// key. A key of the wrong length is refused: the account's address
    /// would be built from it, and no signature could ever unlock what is
    /// sent there.
    pub fn new(index: u8, variant: SpxVariant, pubkey: Vec<u8>) -> Result<Self, String> {
        let param_id: ckb_fips205_utils::ParamId = (variant as u8)
            .try_into()
            .expect("SpxVariant and ParamId share discriminants");
        let (expected, _) = ckb_fips205_utils::verifying::lengths(param_id);
        if pubkey.len() != expected {
            return Err(format!(
                "A {} public key is {} bytes, got {}.",
                variant,
                expected,
                pubkey.len()
            ));
        }
        Ok(Signer {
            index,
            variant,
            pubkey,
        })
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn variant(&self) -> SpxVariant {
        self.variant
    }

    pub fn pubkey(&self) -> &[u8] {
        &self.pubkey
    }
}

/// Single-sig accounts are a degenerate case of multisig: 1/1 msig.
/// With this kind of account, required_first_n being 0 or 1 both make sense.
/// But the two formats hash to different lock script args,
/// so a wallet must consistently use the same one for all accounts it creates.
/// This enum encodes that choice.
///
/// Both formats are valid on-chain but hash to different addresses, so a
/// wallet must consistently use the one its funds were received on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SingleSigConvention {
    /// `<80 01 01 01 param_id>` — the single-sig config documented by the
    /// quantum-resistant lock script. Used by all wallets except v1 imports.
    #[default]
    Standard,
    /// `<80 00 01 01 param_id>` — the config the Quantum Purse v1 web
    /// wallet used (`required_first_n: 0`). Selectable at mnemonic import
    /// so v1 users keep access to their funds.
    /// See https://github.com/quantumpurse/key-vault-wasm/issues/10
    V1,
}

impl SingleSigConvention {
    /// Maps an "importing from the v1 web wallet" flag to its format.
    /// The single place this decision is encoded — UI checkboxes and
    /// CLI flags must all convert through here.
    pub fn new(is_v1: bool) -> Self {
        if is_v1 {
            Self::V1
        } else {
            Self::Standard
        }
    }

    /// The `required_first_n` header byte this format maps to.
    pub fn required_first_n(&self) -> u8 {
        match self {
            Self::Standard => 1,
            Self::V1 => 0,
        }
    }
}

impl fmt::Display for SingleSigConvention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => write!(f, "Standard"),
            Self::V1 => write!(f, "V1"),
        }
    }
}

/// Configuration for the CKB quantum-resistant lock script's all-in-one multisig header.
/// Every account uses this — a "single-sig" account is threshold=1 with one signer.
///
/// The fields are private so that a config can only be built by
/// [`Self::new_multisig`], a checked multisig of 2 or more signers, or
/// [`Self::new_singlesig`], a 1-of-1. Saved accounts and signing requests are
/// read back through the same placing of signers by index and the same checks
/// of the lock script's rules that [`Self::new_multisig`] does. Only the
/// one-co-signer rule is left to [`Self::new_multisig`], since a single-sig
/// account is saved as a 1-of-1.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(try_from = "ReprMultisigConfig")]
pub struct MultisigConfig {
    /// How many of the first N signers must always provide a signature.
    required_first_n: u8,
    /// Minimum number of valid signatures required to unlock (M in M-of-N).
    threshold: u8,
    /// All signers in index order, each with their own SPHINCS+ variant.
    signers: Vec<Signer>,
}

/// A `MultisigConfig` as written in JSON. Reading one goes through the
/// `TryFrom` below, which places the signers by index and checks the lock
/// script's rules, the same as [`MultisigConfig::new_multisig`]. One signer
/// is accepted here: a single-sig account is saved as a 1-of-1.
#[derive(Deserialize)]
struct ReprMultisigConfig {
    required_first_n: u8,
    threshold: u8,
    signers: Vec<Signer>,
}

impl TryFrom<ReprMultisigConfig> for MultisigConfig {
    type Error = String;

    fn try_from(repr: ReprMultisigConfig) -> Result<Self, String> {
        let signers = MultisigConfig::order_signers(repr.signers)?;
        MultisigConfig::pre_validate(&signers, repr.threshold, repr.required_first_n)?;
        Ok(MultisigConfig {
            required_first_n: repr.required_first_n,
            threshold: repr.threshold,
            signers,
        })
    }
}

impl MultisigConfig {
    /// Sorts the signers by index, so the order they arrive in doesn't
    /// matter, and refuses a numbering that doesn't cover 0 to N − 1 exactly
    /// once. The sorted order is what's hashed into the address and what
    /// `required_first_n` counts from, so a gap or repeat would leave who
    /// sits where, and so who is required, undefined.
    fn order_signers(mut signers: Vec<Signer>) -> Result<Vec<Signer>, String> {
        signers.sort_by_key(|signer| signer.index);
        for (place, signer) in signers.iter().enumerate() {
            if usize::from(signer.index) != place {
                return Err(format!(
                    "Signer indices must run from 0 to {}, each used once; got index {} where {} was expected.",
                    signers.len().saturating_sub(1),
                    signer.index,
                    place
                ));
            }
        }
        Ok(signers)
    }

    /// Checks what the lock script requires of any config, a single-sig
    /// 1-of-1 included: 1 to 255 signers (N is one byte), a threshold of 1 to
    /// N, and `required_first_n` at most the threshold. Also refuses a key
    /// listed twice, which would let one person sign as two.
    fn pre_validate(signers: &[Signer], threshold: u8, required_first_n: u8) -> Result<(), String> {
        let total_signers = signers.len();
        if !(1..=255).contains(&total_signers) {
            return Err(format!(
                "A config needs 1..=255 signers, got {}.",
                total_signers
            ));
        }
        if threshold == 0 {
            return Err("Threshold must be at least 1.".to_string());
        }
        if threshold as usize > total_signers {
            return Err(format!(
                "Threshold ({}) exceeds the number of signers ({}).",
                threshold, total_signers
            ));
        }
        if required_first_n > threshold {
            return Err(format!(
                "required_first_n ({}) must not exceed threshold ({}).",
                required_first_n, threshold
            ));
        }
        for (i, a) in signers.iter().enumerate() {
            for b in signers.iter().skip(i + 1) {
                if a.pubkey == b.pubkey {
                    return Err("Duplicate public key in signer list.".to_string());
                }
            }
        }
        Ok(())
    }

    /// Validated constructor. Places the signers by their indices, which must
    /// run from 0 to N − 1 with no gap or repeat, so the order they arrive
    /// in doesn't matter. Returns an error if the config violates on-chain
    /// constraints, or has no co-signer: a multisig account with one signer
    /// would be a single-sig account at another address, which
    /// [`Self::new_singlesig`] makes.
    pub fn new_multisig(
        required_first_n: u8,
        threshold: u8,
        signers: Vec<Signer>,
    ) -> Result<Self, String> {
        if signers.len() < 2 {
            return Err(format!(
                "A multisig account needs 2..=255 signers (at least one co-signer), got {}.",
                signers.len()
            ));
        }
        let signers = Self::order_signers(signers)?;
        Self::pre_validate(&signers, threshold, required_first_n)?;
        Ok(MultisigConfig {
            required_first_n,
            threshold,
            signers,
        })
    }

    /// Convenience constructor for single-signer accounts.
    ///
    /// The quantum-resistant lock script specifies the single-sig
    /// config as `<80 01 01 01 param_id>`: the sole signer is required
    /// (`required_first_n: 1`), with threshold 1 of 1 total. See
    /// <https://github.com/nervosnetwork/quantum-resistant-lock-script/blob/main/docs/simple.md>.
    /// `SingleSigConvention::V1` deviates with `required_first_n: 0`.
    pub fn new_singlesig(
        variant: SpxVariant,
        pubkey: Vec<u8>,
        convention: SingleSigConvention,
    ) -> Self {
        MultisigConfig {
            required_first_n: convention.required_first_n(),
            threshold: 1,
            signers: vec![Signer {
                index: 0,
                variant,
                pubkey,
            }],
        }
    }

    /// How many of the first N signers must always provide a signature (R).
    pub fn required_first_n(&self) -> u8 {
        self.required_first_n
    }

    /// Minimum number of valid signatures required to unlock (M).
    pub fn threshold(&self) -> u8 {
        self.threshold
    }

    /// All signers in index order (N is the count).
    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    /// The 4-byte config header: [S, R, M, N].
    pub fn header_bytes(&self) -> [u8; 4] {
        [
            MULTISIG_RESERVED_FIELD_VALUE,
            self.required_first_n,
            self.threshold,
            self.signers.len() as u8,
        ]
    }

    /// Whether this config represents a single-signer account (1-of-1).
    pub fn is_single_sig(&self) -> bool {
        self.signers.len() == 1 && self.threshold == 1
    }

    /// Maximum byte size of the WitnessArgs lock field for this config.
    ///
    /// Uses the M signers with the largest signatures to ensure the fee
    /// covers any valid M-of-N combination. CKB_TX_MESSAGE_ALL excludes
    /// the lock field entirely, so the signing message is the same
    /// regardless of which M signers participate.
    ///
    /// Single-sig is the 1-of-1 degenerate case of multisig and uses the
    /// same witness layout, so this function handles both uniformly.
    ///
    /// Layout: `[S R M N]` (4 bytes) + N PWOS entries, where each is:
    /// - With signature: 1 (flag) + pubkey_len + sig_len
    /// - Without:        1 (flag) + pubkey_len
    pub fn max_witness_lock_size(&self) -> usize {
        let mut size = 4;
        let mut sig_lens = Vec::with_capacity(self.signers.len());
        for signer in &self.signers {
            let param_id: ckb_fips205_utils::ParamId = (signer.variant as u8)
                .try_into()
                .expect("SpxVariant and ParamId share discriminants");
            let (pk_len, sig_len) = ckb_fips205_utils::verifying::lengths(param_id);
            size += 1 + pk_len;
            sig_lens.push(sig_len);
        }
        sig_lens.sort_unstable_by(|a, b| b.cmp(a));
        size + sig_lens.iter().take(self.threshold as usize).sum::<usize>()
    }

    /// Compute 32-byte lock script args.
    ///
    /// Hashes: `[S R M N] + [param_flag₁ pk₁] + [param_flag₂ pk₂] + ...`
    /// where each param_flag has its lowest bit cleared (no-signature variant).
    pub fn lock_script_args(&self) -> H256 {
        use ckb_fips205_utils::Hasher;
        let mut hasher = Hasher::script_args_hasher();
        hasher.update(&self.header_bytes());
        for signer in &self.signers {
            let param_flag: u8 = (signer.variant as u8) << 1;
            hasher.update(&[param_flag]);
            hasher.update(&signer.pubkey);
        }
        H256(hasher.hash())
    }

    /// This config's lock: the quantum-resistant lock script on mainnet or
    /// testnet, with this config's lock args.
    pub fn lock_script(&self, is_mainnet: bool) -> Script {
        let (code_hash, hash_type) = if is_mainnet {
            (CKB_MAINNET_CODE_HASH, CKB_MAINNET_HASH_TYPE)
        } else {
            (CKB_TESTNET_CODE_HASH, CKB_TESTNET_HASH_TYPE)
        };
        let code_hash: H256 = code_hash
            .trim_start_matches("0x")
            .parse()
            .expect("the lock's code hash constants are valid hex");
        Script::new_builder()
            .code_hash(code_hash.pack())
            .hash_type(hash_type)
            .args(Bytes::from(self.lock_script_args().as_bytes().to_vec()).pack())
            .build()
    }

    /// Picks the M signatures a witness uses from those collected: every
    /// required signer's first, then the others in the order they were
    /// collected. More than M is fine: the wallet that pays signs with up to
    /// M of its keys, and co-signers answer with all of theirs, so a required
    /// co-signer's signature can arrive after M are already in. The extras
    /// are left out. Refuses a duplicate or out-of-range signer, a required
    /// signer with no signature, or fewer than M signatures in all.
    fn pick_signatures<'a>(
        &self,
        signatures: &'a [MsigSignature],
    ) -> Result<Vec<&'a MsigSignature>, String> {
        let n = self.signers().len();
        let m = self.threshold() as usize;
        let r = self.required_first_n() as usize;

        let sig_indices: std::collections::HashSet<usize> =
            signatures.iter().map(|s| s.signer_index).collect();
        if sig_indices.len() != signatures.len() {
            return Err("Duplicate signer indices in signatures.".to_string());
        }

        for i in 0..r {
            if !sig_indices.contains(&i) {
                return Err(format!(
                    "S{} is required (required_first_n={}) but has no signature.",
                    i + 1,
                    r
                ));
            }
        }

        for sig in signatures {
            if sig.signer_index >= n {
                return Err(format!(
                    "Signer index {} is out of range: the multisig has {} signers.",
                    sig.signer_index, n
                ));
            }
        }

        if signatures.len() < m {
            return Err(format!(
                "{} signatures are needed, got {}.",
                m,
                signatures.len()
            ));
        }

        // Every required signer is in, once each, and there are at most M of
        // them, so taking M keeps them all.
        let (required, others): (Vec<_>, Vec<_>) =
            signatures.iter().partition(|s| s.signer_index < r);
        Ok(required.into_iter().chain(others).take(m).collect())
    }

    /// Assembles the complete witness lock field for a multisig transaction.
    ///
    /// Takes the signatures collected, each at its signer's position, and
    /// uses the M that `pick_signatures` picks: the required signers, then
    /// enough of the others.
    ///
    /// Returns the byte vector to be placed in `WitnessArgs.lock`.
    pub fn assemble_multisig_witness(
        &self,
        signatures: &[MsigSignature],
    ) -> Result<Vec<u8>, String> {
        let picked = self.pick_signatures(signatures)?;

        let mut lock = Vec::new();
        lock.extend_from_slice(&self.header_bytes());

        for signer in self.signers() {
            let i = usize::from(signer.index());
            let param_id: ckb_fips205_utils::ParamId = (signer.variant() as u8)
                .try_into()
                .expect("SpxVariant and ParamId share discriminants");

            if let Some(sig) = picked.iter().find(|s| s.signer_index == i) {
                let (_, expected_sig_len) = ckb_fips205_utils::verifying::lengths(param_id);
                if sig.signature.len() != expected_sig_len {
                    return Err(format!(
                        "S{} signature length mismatch: expected {}, got {}.",
                        i + 1,
                        expected_sig_len,
                        sig.signature.len()
                    ));
                }
                lock.push(ckb_fips205_utils::construct_flag(param_id, true));
                lock.extend_from_slice(signer.pubkey());
                lock.extend_from_slice(&sig.signature);
            } else {
                lock.push(ckb_fips205_utils::construct_flag(param_id, false));
                lock.extend_from_slice(signer.pubkey());
            }
        }

        Ok(lock)
    }
}

/// Represents a SPHINCS+ account within the quantum-resistant lock script.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(try_from = "ReprSphincsPlusAccount")]
pub struct SphincsPlusAccount {
    /// Derivation index — used to derive this wallet's SPHINCS+ key pair from the master seed.
    pub index: u32,
    /// Hex-encoded lock script arguments (32-byte blake2b hash of multisig config).
    pub lock_args: String,
    /// Lock script multisig configuration.
    pub config: MultisigConfig,
}

/// A `SphincsPlusAccount` as written in JSON. Reading one goes through the
/// `TryFrom` below, which refuses an account whose lock args are not the hash
/// of its config. Balances are read by the lock args while signing builds
/// the lock from the config, so such an account would show funds it can't
/// spend.
#[derive(Deserialize)]
struct ReprSphincsPlusAccount {
    index: u32,
    lock_args: String,
    config: MultisigConfig,
}

impl TryFrom<ReprSphincsPlusAccount> for SphincsPlusAccount {
    type Error = String;

    fn try_from(repr: ReprSphincsPlusAccount) -> Result<Self, String> {
        let expected = repr.config.lock_script_args().to_string();
        if repr.lock_args != expected {
            return Err(format!(
                "Account {} is saved with lock args {}, but its config hashes to {}.",
                repr.index, repr.lock_args, expected
            ));
        }
        Ok(SphincsPlusAccount {
            index: repr.index,
            lock_args: repr.lock_args,
            config: repr.config,
        })
    }
}

/// One signature in a [`super::SigningResponse`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MsigSignature {
    /// Index of the signer within the multisig config's signers array.
    pub signer_index: usize,
    /// Raw SPHINCS+ signature bytes; hex in the JSON.
    #[serde(with = "hex::serde")]
    pub signature: Vec<u8>,
}

// Visible to the signing tests next door, which build their multisigs with
// the same `key` helper.
#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// SPHINCS+-128 public keys are 32 bytes.
    const SHA2_128S_PK_LEN: usize = 32;

    /// A signer at `index` whose public key is `fill` repeated.
    pub(in crate::types) fn key(index: u8, fill: u8) -> Signer {
        Signer::new(index, SpxVariant::Sha2128S, vec![fill; SHA2_128S_PK_LEN]).unwrap()
    }

    #[test]
    fn signer_needs_a_key_of_its_variants_length() {
        assert!(Signer::new(0, SpxVariant::Sha2128S, vec![0; SHA2_128S_PK_LEN]).is_ok());
        for len in [0, SHA2_128S_PK_LEN - 1, SHA2_128S_PK_LEN + 1] {
            let err = Signer::new(0, SpxVariant::Sha2128S, vec![0; len]).unwrap_err();
            assert!(err.contains(&format!("got {}", len)), "{err}");
        }
    }

    #[test]
    fn multisig_needs_at_least_two_signers() {
        let err = MultisigConfig::new_multisig(0, 1, vec![key(0, 1)]).unwrap_err();
        assert!(err.contains("at least one co-signer"), "{err}");
        assert!(MultisigConfig::new_multisig(0, 1, vec![key(0, 1), key(1, 2)]).is_ok());
    }

    /// The indices must name every place once. A gap, a repeat or a missing
    /// 0 would leave the order, and so who is required, undefined.
    #[test]
    fn multisig_refuses_a_gap_or_repeat_in_signer_indices() {
        for [first, second] in [[0, 2], [0, 0], [1, 2]] {
            let err = MultisigConfig::new_multisig(0, 1, vec![key(first, 1), key(second, 2)])
                .unwrap_err();
            assert!(
                err.contains("Signer indices must run from 0 to 1"),
                "{first}, {second}: {err}"
            );
        }
    }

    /// Signers are placed by index, not by the order they arrive in or by
    /// their keys, so the same indices always give the same address.
    #[test]
    fn multisig_places_signers_by_index() {
        let given =
            MultisigConfig::new_multisig(1, 2, vec![key(0, 9), key(1, 1), key(2, 5)]).unwrap();
        let shuffled =
            MultisigConfig::new_multisig(1, 2, vec![key(2, 5), key(0, 9), key(1, 1)]).unwrap();
        assert_eq!(given.lock_script_args(), shuffled.lock_script_args());
        assert_eq!(
            shuffled.signers()[0].pubkey(),
            vec![9; SHA2_128S_PK_LEN].as_slice()
        );
    }

    /// A saved or received config is placed by index when read, like one
    /// built here, so the order its file lists the signers in doesn't matter
    /// and the address stays the same. An incomplete numbering is refused.
    #[test]
    fn config_reads_back_placed_by_index() {
        let config = MultisigConfig::new_multisig(1, 2, vec![key(0, 1), key(1, 2)]).unwrap();
        let mut json = serde_json::to_value(&config).unwrap();
        json["signers"].as_array_mut().unwrap().swap(0, 1);
        let read: MultisigConfig = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(read.lock_script_args(), config.lock_script_args());
        assert_eq!(read.signers()[0].index(), 0);

        json["signers"][0]["index"] = serde_json::json!(2);
        let err = serde_json::from_value::<MultisigConfig>(json).unwrap_err();
        assert!(
            err.to_string().contains("got index 2 where 1 was expected"),
            "{err}"
        );
    }

    /// A saved or received config must satisfy the lock script's rules when
    /// read, not only when built: a file edited to a threshold above its
    /// signer count, or a key cut short, is refused on load.
    #[test]
    fn config_read_back_is_checked_like_a_new_one() {
        let config = MultisigConfig::new_multisig(1, 2, vec![key(0, 1), key(1, 2)]).unwrap();
        let json = serde_json::to_value(&config).unwrap();

        let mut edited = json.clone();
        edited["threshold"] = serde_json::json!(3);
        let err = serde_json::from_value::<MultisigConfig>(edited).unwrap_err();
        assert!(
            err.to_string()
                .contains("Threshold (3) exceeds the number of signers (2)"),
            "{err}"
        );

        let mut edited = json;
        edited["signers"][0]["pubkey"] =
            serde_json::json!(hex::encode(vec![1; SHA2_128S_PK_LEN - 1]));
        let err = serde_json::from_value::<MultisigConfig>(edited).unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("got {}", SHA2_128S_PK_LEN - 1)),
            "{err}"
        );
    }

    /// Single-sig accounts are saved as 1-of-1 configs, in either
    /// convention, and read back through the same checks as a multisig.
    #[test]
    fn single_sig_config_reads_back() {
        for convention in [SingleSigConvention::Standard, SingleSigConvention::V1] {
            let config = MultisigConfig::new_singlesig(
                SpxVariant::Sha2128S,
                vec![1; SHA2_128S_PK_LEN],
                convention,
            );
            let json = serde_json::to_value(&config).unwrap();
            let read: MultisigConfig = serde_json::from_value(json).unwrap();
            assert_eq!(read.lock_script_args(), config.lock_script_args());
        }
    }

    /// The signer indices `chosen` from signatures collected at `collected`.
    fn chosen(config: &MultisigConfig, collected: &[usize]) -> Result<Vec<usize>, String> {
        let signatures: Vec<MsigSignature> = collected
            .iter()
            .map(|&signer_index| MsigSignature {
                signer_index,
                signature: Vec::new(),
            })
            .collect();
        config
            .pick_signatures(&signatures)
            .map(|chosen| chosen.iter().map(|s| s.signer_index).collect())
    }

    /// A 2-of-3 where S1 must always sign.
    fn s1_required() -> MultisigConfig {
        MultisigConfig::new_multisig(1, 2, vec![key(0, 1), key(1, 2), key(2, 3)]).unwrap()
    }

    /// The wallet that pays signs with S2 and S3, then a co-signer's S1
    /// arrives: three signatures for a threshold of two. The witness takes S1,
    /// then the others in the order they came, and leaves the last out.
    #[test]
    fn more_signatures_than_the_threshold_keep_the_required_first() {
        assert_eq!(chosen(&s1_required(), &[1, 2, 0]), Ok(vec![0, 1]));
        assert_eq!(chosen(&s1_required(), &[2, 1, 0]), Ok(vec![0, 2]));
    }

    #[test]
    fn a_missing_required_signer_or_too_few_signatures_is_refused() {
        let err = chosen(&s1_required(), &[1, 2]).unwrap_err();
        assert!(err.contains("S1 is required"), "{err}");
        let err = chosen(&s1_required(), &[0]).unwrap_err();
        assert!(err.contains("2 signatures are needed, got 1"), "{err}");
    }

    /// A saved account's lock args must be the hash of its saved config. A
    /// file whose config was changed to another valid one is refused, since
    /// its balance and its signatures would belong to different addresses.
    #[test]
    fn account_whose_lock_args_differ_from_its_config_is_refused() {
        let config = MultisigConfig::new_multisig(1, 2, vec![key(0, 1), key(1, 2)]).unwrap();
        let account = SphincsPlusAccount {
            index: 3,
            lock_args: config.lock_script_args().to_string(),
            config,
        };
        let json = serde_json::to_value(&account).unwrap();
        let read: SphincsPlusAccount = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(read.lock_args, account.lock_args);

        let mut edited = json;
        edited["config"]["threshold"] = serde_json::json!(1);
        let err = serde_json::from_value::<SphincsPlusAccount>(edited).unwrap_err();
        assert!(
            err.to_string()
                .contains("Account 3 is saved with lock args"),
            "{err}"
        );
    }
}
