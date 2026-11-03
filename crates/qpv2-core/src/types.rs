use crate::constants::{
    CKB_MAINNET_CODE_HASH, CKB_MAINNET_HASH_TYPE, CKB_TESTNET_CODE_HASH, CKB_TESTNET_HASH_TYPE,
    MULTISIG_RESERVED_FIELD_VALUE,
};
use crate::containers::{SecureString, SecureVec};
use crate::utilities::compute_signing_message;
use ckb_mock_tx_types::{MockInput, ReprMockInput};
use ckb_sdk::{constants::DAO_TYPE_HASH, Address, AddressPayload, NetworkType};
use ckb_types::{
    bytes::Bytes,
    core::{ScriptHashType, TransactionView},
    packed::{CellOutput, Script},
    prelude::*,
    H256,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Shl, Shr};

/// Scrypt param structure.
pub struct ScryptParam {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
    pub len: usize,
}

/// Represents an encrypted payload containing salt, IV, and ciphertext, all hex-encoded.
///
/// **Fields**:
/// - `salt: String` - Hex-encoded salt used for key derivation with Scrypt.
/// - `iv: String` - Hex-encoded initialization vector (nonce) for AES-GCM encryption.
/// - `cipher_text: String` - Hex-encoded encrypted data produced by AES-GCM.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CipherPayload {
    pub salt: String,
    pub iv: String,
    pub cipher_text: String,
}

/// A signer in a multisig group: their SPHINCS+ variant and public key.
///
/// The fields are private so that a signer from outside this crate can only
/// come from [`Signer::new`], which checks the key's length. Saved accounts
/// and signing requests are loaded back as they were written.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Signer {
    variant: SpxVariant,
    #[serde(with = "hex::serde")]
    pubkey: Vec<u8>,
}

impl Signer {
    /// A signer from its parameter set and public key. A key of the wrong
    /// length is refused: the account's address would be built from it, and
    /// no signature could ever unlock what is sent there.
    pub fn new(variant: SpxVariant, pubkey: Vec<u8>) -> Result<Self, String> {
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
        Ok(Signer { variant, pubkey })
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
/// The fields are private so that a config can only be built by [`Self::new`],
/// a checked multisig of 2 or more signers, or [`Self::single_sig`], a 1-of-1.
/// Saved accounts and signing requests are loaded back as they were written.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MultisigConfig {
    /// How many of the first N signers must always provide a signature.
    required_first_n: u8,
    /// Minimum number of valid signatures required to unlock (M in M-of-N).
    threshold: u8,
    /// All signers in deterministic order, each with their own SPHINCS+ variant.
    signers: Vec<Signer>,
}

impl MultisigConfig {
    /// Validate multisig parameters.
    fn pre_validate(signers: &[Signer], threshold: u8, required_first_n: u8) -> Result<(), String> {
        let total_signers = signers.len();
        // total signer must not be 0(impossible), 1(single sig), and ^255(hardcap)
        if !(2..=255).contains(&total_signers) {
            return Err(format!(
                "A multisig account needs 2..=255 signers (at least one co-signer), got {}.",
                total_signers
            ));
        }
        if threshold == 0 {
            return Err("Threshold must be at least 1.".to_string());
        }
        if threshold as usize > total_signers {
            return Err(format!(
                "Threshold ({}) exceeds total signers ({}: {} co-signer(s) + your wallet's key).",
                threshold,
                total_signers,
                total_signers - 1
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

    /// Validated constructor. Returns an error if the config violates on-chain constraints.
    pub fn new(required_first_n: u8, threshold: u8, signers: Vec<Signer>) -> Result<Self, String> {
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
    pub fn single_sig(
        variant: SpxVariant,
        pubkey: Vec<u8>,
        convention: SingleSigConvention,
    ) -> Self {
        MultisigConfig {
            required_first_n: convention.required_first_n(),
            threshold: 1,
            signers: vec![Signer { variant, pubkey }],
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

    /// All signers in deterministic order (N is the count).
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
}

/// Represents a SPHINCS+ account within the quantum-resistant lock script.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SphincsPlusAccount {
    /// Derivation index — used to derive this wallet's SPHINCS+ key pair from the master seed.
    pub index: u32,
    /// Hex-encoded lock script arguments (32-byte blake2b hash of multisig config).
    pub lock_args: String,
    /// Lock script multisig configuration.
    pub config: MultisigConfig,
}

/// Human-readable metadata so co-signers can verify what they're signing.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SigningMetadata {
    /// Sender address (bech32m).
    pub from_address: String,
    /// Recipient address, if applicable.
    pub to_address: Option<String>,
    /// Amount in CKB, if applicable.
    pub amount_ckb: Option<String>,
    /// Transaction type description (e.g. "Transfer", "DAO Deposit").
    pub tx_type: String,
}

/// A request sent to co-signers asking them to sign a transaction.
///
/// Contains the unsigned transaction, pre-computed signing message,
/// and enough context for the co-signer to independently verify and sign.
///
/// The fields are private so that a request can only come from
/// [`Self::new`] or from its JSON, and is never changed afterwards. Both
/// ways run the same checks (the `TryFrom` below), so every request in
/// memory has passed them.
///
/// It holds native CKB types; its JSON form is [`ReprSigningRequest`], the
/// way `MockInput` pairs with `ReprMockInput`.
#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "ReprSigningRequest", into = "ReprSigningRequest")]
pub struct SigningRequest {
    version: u8,
    /// The unsigned CKB transaction.
    unsigned_tx: TransactionView,
    /// The cells the transaction spends, for CKB_TX_MESSAGE_ALL: one per
    /// input, in input order, each naming the input that spends it.
    input_cells: Vec<MockInput>,
    /// The 32-byte signing message (blake2b hash).
    signing_message: H256,
    /// Full multisig configuration for the sending account.
    multisig_config: MultisigConfig,
    /// Witness index in the transaction where the lock field goes.
    script_group_index: usize,
    /// Whether this targets mainnet or testnet.
    is_mainnet: bool,
    /// Human-readable summary for co-signer verification.
    metadata: SigningMetadata,
    /// What the transaction does, decoded from the transaction itself. Not
    /// part of the JSON: it is worked out again whenever a request is read.
    summary: MsigRequestSummary,
}

// By hand: the input cells' type, `MockInput`, has no `Debug`.
impl fmt::Debug for SigningRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningRequest")
            .field("version", &self.version)
            .field("unsigned_tx", &self.unsigned_tx)
            .field(
                "input_cells",
                &format_args!("{} cells", self.input_cells.len()),
            )
            .field("signing_message", &format_args!("{}", self.signing_message))
            .field("multisig_config", &self.multisig_config)
            .field("script_group_index", &self.script_group_index)
            .field("is_mainnet", &self.is_mainnet)
            .field("metadata", &self.metadata)
            .field("summary", &self.summary)
            .finish()
    }
}

/// The JSON form of a [`SigningRequest`]. Reading turns it into a request
/// through the checks in the `TryFrom` below, which `new()` also goes
/// through; writing turns a request back with the `From`. Never use it as a
/// request anywhere else.
#[derive(Serialize, Deserialize)]
struct ReprSigningRequest {
    version: u8,
    unsigned_tx: ckb_jsonrpc_types::Transaction,
    input_cells: Vec<ReprMockInput>,
    /// Hex-encoded.
    signing_message: String,
    multisig_config: MultisigConfig,
    script_group_index: usize,
    is_mainnet: bool,
    metadata: SigningMetadata,
}

impl TryFrom<ReprSigningRequest> for SigningRequest {
    type Error = String;

    /// Checks that the request's multisig is a valid one, that every input
    /// is locked by it, that every cell is plain or a Nervos DAO cell, and
    /// that the transaction hashes to the message it asks to sign.
    ///
    /// The input cells come from the requester too, and that is safe: a
    /// request that lies about a cell hashes differently from what the chain
    /// computes, so a signature made for it cannot unlock anything.
    fn try_from(repr: ReprSigningRequest) -> Result<Self, String> {
        // Rebuilted and referified config from the request.
        let given = &repr.multisig_config;
        let multisig_config = given
            .signers
            .iter()
            .map(|s| Signer::new(s.variant, s.pubkey.clone()))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|signers| {
                MultisigConfig::new(given.required_first_n, given.threshold, signers)
            })
            .map_err(|e| format!("The request's multisig is invalid: {}", e))?;

        let unsigned_tx = ckb_types::packed::Transaction::from(repr.unsigned_tx).into_view();
        let input_cells: Vec<MockInput> =
            repr.input_cells.into_iter().map(MockInput::from).collect();

        // Every input must be locked by this multisig, on the request's
        // network. The signed hash doesn't name the lock, so this signature
        // could also unlock another input under a lock holding the same key,
        // such as one of the co-signer's own single-sig cells.
        let msig_lock = multisig_config.lock_script(repr.is_mainnet);
        if let Some(i) = input_cells
            .iter()
            .position(|mock_input| mock_input.output.lock() != msig_lock)
        {
            return Err(format!(
                "Input cell {} is not locked by the request's multisig.",
                i
            ));
        }

        // Every cell is plain or a Nervos DAO cell, the only kinds this
        // wallet builds. Any other may carry an asset the co-signer can't
        // see, such as a token spent along with its CKB, or a type script
        // that never lets the cell be spent again.
        if let Some(i) = input_cells
            .iter()
            .position(|mock_input| !is_plain_or_dao(&mock_input.output, &mock_input.data))
        {
            return Err(format!(
                "Input cell {} is neither plain nor a Nervos DAO cell.",
                i
            ));
        }
        for (i, (output, data)) in unsigned_tx.outputs_with_data_iter().enumerate() {
            if !is_plain_or_dao(&output, &data) {
                return Err(format!(
                    "Output {} is neither plain nor a Nervos DAO cell.",
                    i
                ));
            }
        }

        let message = compute_signing_message(&unsigned_tx, &input_cells, repr.script_group_index)?;

        let stated = hex::decode(&repr.signing_message)
            .map_err(|e| format!("The request's signing message is not hex: {}.", e))?;
        if stated.as_slice() != message.as_bytes() {
            return Err(
                "The request's signing message does not match its transaction.".to_string(),
            );
        }

        let summary = summarize(&unsigned_tx, &input_cells, &msig_lock, repr.is_mainnet)?;

        Ok(SigningRequest {
            version: repr.version,
            unsigned_tx,
            input_cells,
            signing_message: message,
            multisig_config,
            script_group_index: repr.script_group_index,
            is_mainnet: repr.is_mainnet,
            metadata: repr.metadata,
            summary,
        })
    }
}

/// What a signing request's transaction does, decoded from the transaction
/// itself, for the co-signer to read before signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsigRequestSummary {
    /// The multisig the coins leave, as a full address.
    pub from: String,
    /// One line per output, in output order.
    pub to: Vec<CreatedCell>,
    /// What goes in minus what comes out, in shannons. `None` when the
    /// transaction completes a DAO withdrawal: its payout includes interest,
    /// which only the deposit's headers tell.
    pub fee: Option<u64>,
}

/// One output of a signing request's transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedCell {
    pub kind: CellKind,
    /// Who receives the cell: whoever can spend it next.
    pub recipient: Recipient,
    pub shannons: u64,
}

/// The kinds of cell a signing request may create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// No type script and no data: plain CKB.
    Plain,
    /// A new Nervos DAO deposit.
    DaoDeposit,
    /// A Nervos DAO deposit moved on to its withdrawal request.
    DaoWithdrawalRequest,
}

/// Who receives a created cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recipient {
    /// Receiver of the change.
    Sender,
    /// Anyone else, as a full address.
    Other(String),
}

/// Decodes what `tx` does. `lock` is the request's multisig lock, which
/// every input already carries.
fn summarize(
    tx: &TransactionView,
    input_cells: &[MockInput],
    msig_lock: &Script,
    is_mainnet: bool,
) -> Result<MsigRequestSummary, String> {
    let mut to = Vec::new();
    let mut output_capacity: u64 = 0;
    for (output, data) in tx.outputs_with_data_iter() {
        let shannons: u64 = output.capacity().unpack();
        output_capacity = output_capacity
            .checked_add(shannons)
            .ok_or_else(|| "The request's outputs add up to more CKB than exists.".to_string())?;
        // Only plain and DAO cells.
        let kind = if output.type_().to_opt().is_none() {
            CellKind::Plain
        } else if data.iter().all(|byte| *byte == 0) {
            CellKind::DaoDeposit
        } else {
            CellKind::DaoWithdrawalRequest
        };
        let recipient = if output.lock() == *msig_lock {
            Recipient::Sender
        } else {
            Recipient::Other(address_of(&output.lock(), is_mainnet))
        };
        to.push(CreatedCell {
            kind,
            recipient,
            shannons,
        });
    }

    let mut input_capacity: u64 = 0;
    let mut completes_withdrawal = false;
    for mock_input in input_cells {
        let shannons: u64 = mock_input.output.capacity().unpack();
        input_capacity = input_capacity
            .checked_add(shannons)
            .ok_or_else(|| "The request's inputs add up to more CKB than exists.".to_string())?;
        // before this function is executed, try_from() implemetnation has already filtered only
        // plain cell and DAO cell. So if the input is not a plain cell, it must be a DAO cell.
        let is_dao = mock_input.output.type_().to_opt().is_some();
        if is_dao && mock_input.data.iter().any(|byte| *byte != 0) {
            completes_withdrawal = true;
        }
    }
    let fee = if completes_withdrawal {
        None
    } else {
        let fee = input_capacity
            .checked_sub(output_capacity)
            .ok_or_else(|| "The request's outputs hold more CKB than its inputs.".to_string())?;
        Some(fee)
    };

    Ok(MsigRequestSummary {
        from: address_of(msig_lock, is_mainnet),
        to,
        fee,
    })
}

/// A lock as its full CKB address.
///
/// `AddressPayload::from` panics on a hash type CKB doesn't know, which a
/// request can't carry: the JSON reader only accepts the known ones.
fn address_of(lock: &Script, is_mainnet: bool) -> String {
    let network = if is_mainnet {
        NetworkType::Mainnet
    } else {
        NetworkType::Testnet
    };
    Address::new(network, AddressPayload::from(lock.clone()), true).to_string()
}

/// Whether a cell is plain (no type script, no data) or a Nervos DAO cell
/// (the DAO's type script and 8 bytes of data).
fn is_plain_or_dao(output: &CellOutput, data: &[u8]) -> bool {
    match output.type_().to_opt() {
        None => data.is_empty(),
        Some(type_script) => {
            let dao = Script::new_builder()
                .code_hash(DAO_TYPE_HASH.pack())
                .hash_type(ScriptHashType::Type)
                .build();
            type_script == dao && data.len() == 8
        }
    }
}

impl From<SigningRequest> for ReprSigningRequest {
    fn from(request: SigningRequest) -> Self {
        ReprSigningRequest {
            version: request.version,
            unsigned_tx: request.unsigned_tx.data().into(),
            input_cells: request
                .input_cells
                .into_iter()
                .map(ReprMockInput::from)
                .collect(),
            signing_message: request.signing_message.to_string(),
            multisig_config: request.multisig_config,
            script_group_index: request.script_group_index,
            is_mainnet: request.is_mainnet,
            metadata: request.metadata,
        }
    }
}

impl SigningRequest {
    /// Builds a `SigningRequest` from an unsigned transaction and its context.
    ///
    /// Computes the CKB_TX_MESSAGE_ALL signing message and packages everything
    /// a co-signer needs to verify and sign the transaction. The result goes
    /// through the same checks as a request read from JSON, so what this
    /// wallet builds is always what a co-signer can read.
    pub fn new(
        tx: &TransactionView,
        input_cells: &[MockInput],
        config: &MultisigConfig,
        script_group_index: usize,
        is_mainnet: bool,
        metadata: SigningMetadata,
    ) -> Result<Self, String> {
        let signing_message = compute_signing_message(tx, input_cells, script_group_index)?;

        let unsigned_tx: ckb_jsonrpc_types::Transaction = tx.data().into();

        let spent: Vec<ReprMockInput> = input_cells
            .iter()
            .cloned()
            .map(ReprMockInput::from)
            .collect();

        Self::try_from(ReprSigningRequest {
            version: 1,
            unsigned_tx,
            input_cells: spent,
            signing_message: signing_message.to_string(),
            multisig_config: config.clone(),
            script_group_index,
            is_mainnet,
            metadata,
        })
    }

    /// Format version of the request.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The unsigned CKB transaction.
    pub fn unsigned_tx(&self) -> &TransactionView {
        &self.unsigned_tx
    }

    /// The 32-byte signing message (blake2b hash).
    pub fn signing_message(&self) -> &H256 {
        &self.signing_message
    }

    /// Full multisig configuration for the sending account.
    pub fn multisig_config(&self) -> &MultisigConfig {
        &self.multisig_config
    }

    /// Witness index in the transaction where the lock field goes.
    pub fn script_group_index(&self) -> usize {
        self.script_group_index
    }

    /// Whether this targets mainnet or testnet.
    pub fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }

    /// Human-readable summary for co-signer verification.
    pub fn metadata(&self) -> &SigningMetadata {
        &self.metadata
    }

    /// What the transaction does, decoded from the transaction itself.
    pub fn summary(&self) -> &MsigRequestSummary {
        &self.summary
    }
}

/// A co-signer's response: every signature their wallet made for a request,
/// one per key it holds in the multisig.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SigningResponse {
    pub version: u8,
    pub signed: Vec<MsigSignature>,
    /// Echo of the signing message this was signed against, for cross-check.
    pub signing_message: String,
}

impl SigningResponse {
    /// Checks a co-signer's response against the request it answers and
    /// against `collected`, the signatures gathered so far, before the caller
    /// adds its signatures. The GUI and the CLI both use it. The signatures
    /// themselves are not verified here: a wrong one is refused by the chain.
    pub fn validate(
        &self,
        request: &SigningRequest,
        collected: &[MsigSignature],
    ) -> Result<(), String> {
        if self.version != request.version {
            return Err(format!(
                "Response version {} does not match the request's {}.",
                self.version, request.version
            ));
        }
        if self.signing_message != request.signing_message.to_string() {
            return Err("Response signing message does not match.".to_string());
        }

        // A response carries one signature per key its wallet holds. Every
        // one is checked before any is added.
        if self.signed.is_empty() {
            return Err("The response holds no signature.".to_string());
        }
        let n_signers = request.multisig_config.signers().len();
        for (i, sig) in self.signed.iter().enumerate() {
            if sig.signer_index >= n_signers {
                return Err(format!(
                    "Signer index {} out of range (N={}).",
                    sig.signer_index, n_signers
                ));
            }
            if collected.iter().any(|s| s.signer_index == sig.signer_index) {
                return Err(format!(
                    "Signer {} already provided a signature.",
                    sig.signer_index
                ));
            }
            if self.signed[..i]
                .iter()
                .any(|s| s.signer_index == sig.signer_index)
            {
                return Err(format!(
                    "The response holds signer {} twice.",
                    sig.signer_index
                ));
            }
        }
        Ok(())
    }
}

/// One signature in a [`SigningResponse`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MsigSignature {
    /// Index of the signer within the multisig config's signers array.
    pub signer_index: usize,
    /// Raw SPHINCS+ signature bytes; hex in the JSON.
    #[serde(with = "hex::serde")]
    pub signature: Vec<u8>,
}

/// Authentication method used to protect the vault.
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(tag = "type")]
pub enum AuthMethod {
    /// Password-based authentication using scrypt key derivation.
    #[default]
    Password,
    /// Platform credential store (macOS Keychain, Windows TPM, Linux TPM).
    Keychain,
    /// FIDO2 hardware key with hmac-secret extension.
    Fido2 { credential_id: String },
    /// Watch-only wallet backed by a Trezor hardware device. The private key
    /// never leaves the device; there is no vault seed, and signing is done by
    /// streaming the transaction to the device. `model` is a display label
    /// (e.g. "Trezor Safe 7").
    Trezor { model: String },
}

/// Authentication key used to encrypt/decrypt the vault.
/// Unifies password-based and crypto key paths so that all core functions
/// accept a single parameter regardless of how the key was obtained.
pub enum AuthKey {
    /// Password to be hashed with Scrypt before use as AES-256 key.
    Password(SecureString),
    /// Pre-derived 32-byte AES-256 key (e.g. from passkey PRF + HKDF).
    CryptoKey(SecureVec),
}

/// Lightweight wallet listing entry derived from scanning the filesystem.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalletEntry {
    pub id: u32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalletInfo {
    #[serde(default)]
    pub name: String,
    pub spx_variant: SpxVariant,
    #[serde(default)]
    pub auth_method: AuthMethod,
    /// Single-sig header convention for every account in this wallet.
    #[serde(default)]
    pub single_sig_convention: SingleSigConvention,
}

/// ID of all 12 SPHINCS+ variants following https://github.com/cryptape/quantum-resistant-lock-script/pull/14
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpxVariant {
    Sha2128F = 48,
    Sha2128S,
    Sha2192F,
    Sha2192S,
    Sha2256F,
    Sha2256S,
    Shake128F,
    Shake128S,
    Shake192F,
    Shake192S,
    Shake256F,
    Shake256S,
}

impl SpxVariant {
    // Each seed in the SPHINCS+ input seed trio {sk_seed, sk_prf, pk_seed} needs this amount of entropy in byte
    pub fn required_entropy_size_component(&self) -> usize {
        match self {
            Self::Sha2128F | Self::Sha2128S | Self::Shake128F | Self::Shake128S => 16,
            Self::Sha2192F | Self::Sha2192S | Self::Shake192F | Self::Shake192S => 24,
            _ => 32,
        }
    }

    // The whole SPHINCS+ seed backup seed/ the trio {sk_seed, sk_prf, pk_seed} needs this much of entropy in byte
    pub fn required_entropy_size_total(&self) -> usize {
        self.required_entropy_size_component() * 3
    }

    // Mapping each SPHINCS+ variant to the corresponding bip39 type (differentiated by word count)
    // Each word count option below contain the corresponding entropy defined in `required_entropy_size_component`
    pub fn required_bip39_size_in_word_component(&self) -> usize {
        match self {
            Self::Sha2128F | Self::Sha2128S | Self::Shake128F | Self::Shake128S => 12,
            Self::Sha2192F | Self::Sha2192S | Self::Shake192F | Self::Shake192S => 18,
            _ => 24,
        }
    }

    // The whole SPHINCS+ seed backup seed/ the trio {sk_seed, sk_prf, pk_seed} will need this much of words in BIP39 standard
    pub fn required_bip39_size_in_word_total(&self) -> usize {
        self.required_bip39_size_in_word_component() * 3
    }
}

impl fmt::Display for SpxVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SpxVariant::Sha2128F => "Sha2128F",
            SpxVariant::Sha2128S => "Sha2128S",
            SpxVariant::Sha2192F => "Sha2192F",
            SpxVariant::Sha2192S => "Sha2192S",
            SpxVariant::Sha2256F => "Sha2256F",
            SpxVariant::Sha2256S => "Sha2256S",
            SpxVariant::Shake128F => "Shake128F",
            SpxVariant::Shake128S => "Shake128S",
            SpxVariant::Shake192F => "Shake192F",
            SpxVariant::Shake192S => "Shake192S",
            SpxVariant::Shake256F => "Shake256F",
            SpxVariant::Shake256S => "Shake256S",
        };
        write!(f, "{}", s)
    }
}

impl Shr<u8> for SpxVariant {
    type Output = u8;
    fn shr(self, rhs: u8) -> u8 {
        (self as u8) >> rhs
    }
}

impl Shl<u8> for SpxVariant {
    type Output = u8;
    fn shl(self, rhs: u8) -> u8 {
        (self as u8) << rhs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPHINCS+-128 public keys are 32 bytes.
    const SHA2_128S_PK_LEN: usize = 32;

    #[test]
    fn signer_needs_a_key_of_its_variants_length() {
        assert!(Signer::new(SpxVariant::Sha2128S, vec![0; SHA2_128S_PK_LEN]).is_ok());
        for len in [0, SHA2_128S_PK_LEN - 1, SHA2_128S_PK_LEN + 1] {
            let err = Signer::new(SpxVariant::Sha2128S, vec![0; len]).unwrap_err();
            assert!(err.contains(&format!("got {}", len)), "{err}");
        }
    }

    #[test]
    fn multisig_needs_at_least_two_signers() {
        let one = Signer::new(SpxVariant::Sha2128S, vec![1; SHA2_128S_PK_LEN]).unwrap();
        let err = MultisigConfig::new(0, 1, vec![one.clone()]).unwrap_err();
        assert!(err.contains("at least one co-signer"), "{err}");
        let two = Signer::new(SpxVariant::Sha2128S, vec![2; SHA2_128S_PK_LEN]).unwrap();
        assert!(MultisigConfig::new(0, 1, vec![one, two]).is_ok());
    }

    mod signing_request {
        use super::*;
        use crate::constants::{CKB_TESTNET_CODE_HASH, CKB_TESTNET_HASH_TYPE};
        use ckb_types::{
            bytes::Bytes,
            core::TransactionBuilder,
            packed::{CellInput, CellOutput, OutPoint, Script, WitnessArgs},
        };

        const CKB: u64 = 100_000_000;

        fn multisig() -> MultisigConfig {
            let signers = vec![
                Signer::new(SpxVariant::Sha2128S, vec![1; SHA2_128S_PK_LEN]).unwrap(),
                Signer::new(SpxVariant::Sha2128S, vec![2; SHA2_128S_PK_LEN]).unwrap(),
            ];
            MultisigConfig::new(0, 1, signers).unwrap()
        }

        /// The multisig's lock on testnet.
        fn lock_of(config: &MultisigConfig) -> Script {
            let code_hash: H256 = CKB_TESTNET_CODE_HASH
                .trim_start_matches("0x")
                .parse()
                .unwrap();
            Script::new_builder()
                .code_hash(code_hash.pack())
                .hash_type(CKB_TESTNET_HASH_TYPE)
                .args(Bytes::from(config.lock_script_args().as_bytes().to_vec()).pack())
                .build()
        }

        fn cell(capacity_ckb: u64, lock: Script) -> CellOutput {
            CellOutput::new_builder()
                .capacity(capacity_ckb * CKB)
                .lock(lock)
                .build()
        }

        /// A cell with no type script and no data.
        fn plain_cell(capacity_ckb: u64, lock: Script) -> (CellOutput, Bytes) {
            (cell(capacity_ckb, lock), Bytes::new())
        }

        /// A Nervos DAO cell: a deposit when `block` is 0, otherwise a
        /// withdrawal request for the deposit made in that block.
        fn dao_cell(capacity_ckb: u64, lock: Script, block: u64) -> (CellOutput, Bytes) {
            let dao_type = Script::new_builder()
                .code_hash(DAO_TYPE_HASH.pack())
                .hash_type(ScriptHashType::Type)
                .build();
            let output = cell(capacity_ckb, lock)
                .as_builder()
                .type_(Some(dao_type).pack())
                .build();
            (output, Bytes::from(block.to_le_bytes().to_vec()))
        }

        /// A token cell: some other type script, with the token amount as data.
        fn token_cell(capacity_ckb: u64, lock: Script) -> (CellOutput, Bytes) {
            let token_type = Script::new_builder()
                .code_hash([7u8; 32].pack())
                .hash_type(ScriptHashType::Type)
                .build();
            let output = cell(capacity_ckb, lock)
                .as_builder()
                .type_(Some(token_type).pack())
                .build();
            (output, Bytes::from(1_000_000u128.to_le_bytes().to_vec()))
        }

        /// A transaction spending `inputs` cells into `outputs`.
        fn transaction(inputs: u32, outputs: Vec<(CellOutput, Bytes)>) -> TransactionView {
            let mut builder = TransactionBuilder::default();
            for index in 0..inputs {
                builder = builder.input(CellInput::new(OutPoint::new([1u8; 32].pack(), index), 0));
            }
            for (output, data) in outputs {
                builder = builder.output(output).output_data(data.pack());
            }
            builder
                .witness(
                    WitnessArgs::new_builder()
                        .lock(Some(Bytes::from(vec![0u8; 16])).pack())
                        .build()
                        .as_bytes()
                        .pack(),
                )
                .build()
        }

        /// A testnet request from `config`, spending `inputs` into `outputs`.
        fn request(
            inputs: Vec<(CellOutput, Bytes)>,
            outputs: Vec<(CellOutput, Bytes)>,
            config: &MultisigConfig,
        ) -> Result<SigningRequest, String> {
            let tx = transaction(inputs.len() as u32, outputs);
            let input_cells: Vec<MockInput> = tx
                .inputs()
                .into_iter()
                .zip(inputs)
                .map(|(input, (output, data))| MockInput {
                    input,
                    output,
                    data,
                    header: None,
                })
                .collect();
            let metadata = SigningMetadata {
                from_address: String::new(),
                to_address: None,
                amount_ckb: None,
                tx_type: "Transfer".to_string(),
            };
            SigningRequest::new(&tx, &input_cells, config, 0, false, metadata)
        }

        /// A request to send 100 CKB to someone out of plain `cells`.
        fn spend(
            cells: Vec<CellOutput>,
            config: &MultisigConfig,
        ) -> Result<SigningRequest, String> {
            let inputs = cells.into_iter().map(|c| (c, Bytes::new())).collect();
            request(inputs, vec![plain_cell(100, Script::default())], config)
        }

        /// A request to send 100 CKB out of a 200 CKB cell held by the
        /// multisig, as JSON so a test can tamper with it.
        fn request_json() -> serde_json::Value {
            let config = multisig();
            let request = spend(vec![cell(200, lock_of(&config))], &config).unwrap();
            serde_json::to_value(request).unwrap()
        }

        fn read(json: serde_json::Value) -> Result<SigningRequest, String> {
            serde_json::from_value(json).map_err(|e| e.to_string())
        }

        #[test]
        fn request_written_to_json_reads_back_unchanged() {
            let json = request_json();
            let request = read(json.clone()).unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), json);
        }

        #[test]
        fn request_missing_an_input_cell_is_refused() {
            let mut json = request_json();
            json["input_cells"].as_array_mut().unwrap().pop();
            let err = read(json).unwrap_err();
            assert!(
                err.contains("Expected one input cell per transaction input (1), got 0"),
                "{err}"
            );
        }

        #[test]
        fn request_with_a_cell_naming_another_input_is_refused() {
            let mut json = request_json();
            json["input_cells"][0]["input"]["previous_output"]["index"] = "0x1".into();
            let err = read(json).unwrap_err();
            assert!(
                err.contains("Input cell 0 does not name the transaction's input 0"),
                "{err}"
            );
        }

        #[test]
        fn request_whose_transaction_was_swapped_is_refused() {
            // Same message, but the transaction now sends 150 CKB.
            let mut json = request_json();
            let swapped: ckb_jsonrpc_types::Transaction =
                transaction(1, vec![plain_cell(150, Script::default())])
                    .data()
                    .into();
            json["unsigned_tx"] = serde_json::to_value(swapped).unwrap();
            let err = read(json).unwrap_err();
            assert!(err.contains("does not match its transaction"), "{err}");
        }

        /// The drain attack: one input from the multisig, one from the
        /// co-signer's own single-sig account. The hash is honest, but it
        /// doesn't name the lock, so the signature could also unlock the
        /// co-signer's own cell.
        #[test]
        fn request_spending_a_cell_outside_its_multisig_is_refused() {
            let config = multisig();
            let own_key = config.signers()[0].clone();
            let own_account =
                MultisigConfig::single_sig(own_key.variant, own_key.pubkey, Default::default());
            let cells = vec![
                cell(100, lock_of(&config)),
                cell(100_000, lock_of(&own_account)),
            ];
            let err = spend(cells, &config).unwrap_err();
            assert!(
                err.contains("Input cell 1 is not locked by the request's multisig"),
                "{err}"
            );
        }

        /// A 1-of-1 "multisig" is the co-signer's own single-sig lock, so its
        /// coins would pass the lock check.
        #[test]
        fn request_whose_multisig_is_a_single_sig_is_refused() {
            let own_key = multisig().signers()[0].clone();
            let own_account =
                MultisigConfig::single_sig(own_key.variant, own_key.pubkey, Default::default());
            let err = spend(vec![cell(200, lock_of(&own_account))], &own_account).unwrap_err();
            assert!(
                err.contains("The request's multisig is invalid: A multisig account needs 2..=255"),
                "{err}"
            );
        }

        /// A token cell the multisig holds, spent along with its CKB: the
        /// co-signer would see only the CKB.
        #[test]
        fn request_spending_a_token_cell_is_refused() {
            let config = multisig();
            let err = request(
                vec![token_cell(200, lock_of(&config))],
                vec![plain_cell(100, Script::default())],
                &config,
            )
            .unwrap_err();
            assert!(
                err.contains("Input cell 0 is neither plain nor a Nervos DAO cell"),
                "{err}"
            );
        }

        /// The multisig's ordinary transfers pass: plain cells in, plain
        /// cells out.
        #[test]
        fn request_spending_plain_cells_passes() {
            let config = multisig();
            let result = request(
                vec![plain_cell(200, lock_of(&config))],
                vec![
                    plain_cell(100, Script::default()),
                    plain_cell(99, lock_of(&config)),
                ],
                &config,
            );
            assert!(result.is_ok(), "{:?}", result.err());
        }

        /// The multisig's own DAO steps still pass: a withdrawal request
        /// spends a deposit, with a plain cell paying the fee.
        #[test]
        fn request_for_a_dao_withdrawal_request_passes() {
            let config = multisig();
            let result = request(
                vec![
                    dao_cell(200, lock_of(&config), 0),
                    plain_cell(100, lock_of(&config)),
                ],
                vec![
                    dao_cell(200, lock_of(&config), 12_345),
                    plain_cell(99, lock_of(&config)),
                ],
                &config,
            );
            assert!(result.is_ok(), "{:?}", result.err());
        }

        /// A friend's single-sig lock, for outputs that leave the multisig.
        fn friend() -> Script {
            let key = Signer::new(SpxVariant::Sha2128S, vec![3; SHA2_128S_PK_LEN]).unwrap();
            lock_of(&MultisigConfig::single_sig(
                key.variant,
                key.pubkey,
                SingleSigConvention::Standard,
            ))
        }

        fn address(lock: &Script) -> String {
            address_of(lock, false)
        }

        #[test]
        fn transfer_reads_as_a_send_with_change_and_fee() {
            let config = multisig();
            let summary = request(
                vec![plain_cell(200, lock_of(&config))],
                vec![plain_cell(100, friend()), plain_cell(99, lock_of(&config))],
                &config,
            )
            .unwrap()
            .summary()
            .clone();
            assert_eq!(
                summary,
                MsigRequestSummary {
                    from: address(&lock_of(&config)),
                    to: vec![
                        CreatedCell {
                            kind: CellKind::Plain,
                            recipient: Recipient::Other(address(&friend())),
                            shannons: 100 * CKB,
                        },
                        CreatedCell {
                            kind: CellKind::Plain,
                            recipient: Recipient::Sender,
                            shannons: 99 * CKB,
                        },
                    ],
                    fee: Some(CKB),
                }
            );
        }

        /// SigningRequest construction validation should compare the whole lock
        #[test]
        fn summarize_compares_the_whole_lock() {
            let config = multisig();
            let lookalike = lock_of(&config)
                .as_builder()
                .code_hash([9u8; 32].pack())
                .build();
            let summary = request(
                vec![plain_cell(200, lock_of(&config))],
                vec![plain_cell(199, lookalike.clone())],
                &config,
            )
            .unwrap()
            .summary()
            .clone();
            assert_eq!(
                summary.to[0].recipient,
                Recipient::Other(address(&lookalike))
            );
        }

        #[test]
        fn dao_deposit_for_a_friend_names_the_friend() {
            let config = multisig();
            let summary = request(
                vec![plain_cell(200, lock_of(&config))],
                vec![dao_cell(199, friend(), 0)],
                &config,
            )
            .unwrap()
            .summary()
            .clone();
            assert_eq!(
                summary.to[0],
                CreatedCell {
                    kind: CellKind::DaoDeposit,
                    recipient: Recipient::Other(address(&friend())),
                    shannons: 199 * CKB,
                }
            );
        }
    }
}
