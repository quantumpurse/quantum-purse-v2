use clap::{Parser, Subcommand};
use qpv2_core::types::{AuthKey, AuthMethod, SpxVariant};
use qpv2_core::KeyVault;
use qpv2_core::SecureString;
use rpassword::read_password;
use std::fs;
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "qpv2")]
#[command(about = "A SPHINCS+-based key management CLI with integrated CKB blockchain address resolution.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new vault by generating a seed
    Init {
        /// SPHINCS+ variant (Sha2128F, Sha2128S, Sha2192F, Sha2192S, Sha2256F, Sha2256S, Shake128F, Shake128S, Shake192F, Shake192S, Shake256F, Shake256S)
        #[arg(short, long)]
        variant: String,
        /// Use platform credential store (Touch ID on macOS, Credential Manager on Windows, Secret Service on Linux)
        #[arg(long)]
        keychain: bool,
        /// Use a FIDO2 security key with hmac-secret extension
        #[arg(long)]
        fido2: bool,
    },
    /// Mnemonic operations (import/export)
    Mnemonic {
        #[command(subcommand)]
        command: MnemonicCommands,
    },
    /// Account operations (new/list/recover/try-gen-batch)
    Account {
        #[command(subcommand)]
        command: AccountCommands,
    },
    /// Generate raw SPHINCS+ signature for any message (returns signature and public key)
    Sign {
        /// Account identifier (CKB quantum lock args). Run `qpv2-cli account list` to see all accounts
        #[arg(short, long)]
        identifier: String,
        /// Message to sign (hex-encoded)
        #[arg(short, long)]
        message: String,
    },
    /// Verify raw SPHINCS+ signature
    Verify {
        /// SPHINCS+ variant (Sha2128F, Sha2128S, Sha2192F, Sha2192S, Sha2256F, Sha2256S, Shake128F, Shake128S, Shake192F, Shake192S, Shake256F, Shake256S)
        #[arg(short, long)]
        variant: String,
        /// Public key (hex-encoded)
        #[arg(short, long)]
        public_key: String,
        /// Message to verify (hex-encoded)
        #[arg(short, long)]
        message: String,
        /// Signature to verify (hex-encoded)
        #[arg(short, long)]
        signature: String,
    },
    /// CKB blockchain operations (sign/get-tx-message)
    Ckb {
        #[command(subcommand)]
        command: CkbCommands,
    },
    /// Clear all vault data
    Clear,
    /// Display vault information
    Info,
}

#[derive(Subcommand)]
enum MnemonicCommands {
    Import {
        /// SPHINCS+ variant
        #[arg(short, long)]
        variant: String,

        #[arg(short, long)]
        seed_file: Option<String>,

        /// Use platform credential store instead of password
        #[arg(long)]
        keychain: bool,

        /// Use a FIDO2 security key with hmac-secret extension
        #[arg(long)]
        fido2: bool,
    },
    Export {
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum AccountCommands {
    /// Generate a new SPHINCS+ account
    New,
    /// List all SPHINCS+ accounts
    List,
    /// Recover accounts
    Recover {
        /// Number of accounts to recover
        #[arg(short, long)]
        count: u32,
    },
    /// Generate account batch (for discovery)
    TryGenBatch {
        /// Start index
        #[arg(short, long)]
        start: u32,
        /// Count
        #[arg(short, long)]
        count: u32,
    },
}

#[derive(Subcommand)]
enum CkbCommands {
    /// Sign a CKB message
    Sign {
        /// CKB quantum lock script args (Account identifier)
        #[arg(short, long)]
        lock_args: String,
        /// Message to sign (hex-encoded)
        #[arg(short, long)]
        message: String,
    },
    /// Get CKB transaction message hash from mock transaction
    GetTxMessage {
        /// Path to serialized mock transaction file
        #[arg(short, long)]
        tx_file: String,
    },
}

fn parse_variant(variant_str: &str) -> Result<SpxVariant, String> {
    match variant_str.to_lowercase().as_str() {
        "sha2128f" => Ok(SpxVariant::Sha2128F),
        "sha2128s" => Ok(SpxVariant::Sha2128S),
        "sha2192f" => Ok(SpxVariant::Sha2192F),
        "sha2192s" => Ok(SpxVariant::Sha2192S),
        "sha2256f" => Ok(SpxVariant::Sha2256F),
        "sha2256s" => Ok(SpxVariant::Sha2256S),
        "shake128f" => Ok(SpxVariant::Shake128F),
        "shake128s" => Ok(SpxVariant::Shake128S),
        "shake192f" => Ok(SpxVariant::Shake192F),
        "shake192s" => Ok(SpxVariant::Shake192S),
        "shake256f" => Ok(SpxVariant::Shake256F),
        "shake256s" => Ok(SpxVariant::Shake256S),
        _ => Err(format!("Invalid variant: {}", variant_str)),
    }
}

fn prompt_for_input(prompt: &str) -> Result<SecureString, String> {
    print!("{}", prompt);
    io::stdout().flush().map_err(|e| e.to_string())?;
    let input = read_password().map_err(|e| e.to_string())?;
    let result = SecureString::from_utf8(input.into_bytes())?;
    Ok(result)
}

fn get_auth_key() -> Result<AuthKey, String> {
    let wallet_info = KeyVault::read_wallet_info()?;
    match wallet_info.auth_method {
        AuthMethod::Password => {
            let password = prompt_for_input("Enter password: ")?;
            Ok(AuthKey::Password(password))
        }
        AuthMethod::Keychain => {
            println!("Authenticate with {}...", keychain::keystore_short_name());
            let key = keychain::retrieve_key()?;
            Ok(AuthKey::CryptoKey(key))
        }
        AuthMethod::Fido2 { ref credential_id } => {
            let cred_bytes = hex::decode(credential_id)
                .map_err(|e| format!("Invalid credential ID: {}", e))?;
            let pin = prompt_for_input("Enter security key PIN: ")?;
            let hmac_output = keychain::fido2::authenticate(&cred_bytes, &pin)?;
            let key = qpv2_core::utilities::derive_vault_enc_key(&hmac_output)?;
            Ok(AuthKey::CryptoKey(key))
        }
    }
}

fn init_with_keychain(vault: &KeyVault) -> Result<(), String> {
    let key = qpv2_core::utilities::get_random_bytes(32)
        .map_err(|e| format!("Failed to generate key: {}", e))?;
    keychain::store_key(&key)?;
    if let Err(e) = vault.generate_master_seed(AuthKey::CryptoKey(key), AuthMethod::Keychain) {
        let _ = keychain::delete_key();
        return Err(e);
    }
    Ok(())
}

fn import_with_keychain(vault: &KeyVault, seed_phrase: SecureString) -> Result<(), String> {
    let key = qpv2_core::utilities::get_random_bytes(32)
        .map_err(|e| format!("Failed to generate key: {}", e))?;
    keychain::store_key(&key)?;
    if let Err(e) =
        vault.import_seed_phrase(seed_phrase, AuthKey::CryptoKey(key), AuthMethod::Keychain)
    {
        let _ = keychain::delete_key();
        return Err(e);
    }
    Ok(())
}

fn init_with_fido2(vault: &KeyVault) -> Result<(), String> {
    let pin = prompt_for_input("Enter security key PIN: ")?;
    println!("Registering credential...");
    let credential = keychain::fido2::register(&pin)?;
    let credential_id = hex::encode(&credential.credential_id);

    println!("Deriving encryption key...");
    let hmac_output = keychain::fido2::authenticate(&credential.credential_id, &pin)?;
    let key = qpv2_core::utilities::derive_vault_enc_key(&hmac_output)?;

    let auth_method = AuthMethod::Fido2 { credential_id };
    vault.generate_master_seed(AuthKey::CryptoKey(key), auth_method)?;
    Ok(())
}

fn import_with_fido2(vault: &KeyVault, seed_phrase: SecureString) -> Result<(), String> {
    let pin = prompt_for_input("Enter security key PIN: ")?;
    println!("Registering credential...");
    let credential = keychain::fido2::register(&pin)?;
    let credential_id = hex::encode(&credential.credential_id);

    println!("Deriving encryption key...");
    let hmac_output = keychain::fido2::authenticate(&credential.credential_id, &pin)?;
    let key = qpv2_core::utilities::derive_vault_enc_key(&hmac_output)?;

    let auth_method = AuthMethod::Fido2 { credential_id };
    vault.import_seed_phrase(seed_phrase, AuthKey::CryptoKey(key), auth_method)?;
    Ok(())
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            variant,
            keychain,
            fido2,
        } => {
            let variant = parse_variant(&variant)?;
            let vault = KeyVault::new(variant);

            println!("Initializing wallet with variant: {}", variant);
            println!(
                "Required mnemonic words: {}",
                variant.required_bip39_size_in_word_total()
            );

            if keychain && fido2 {
                return Err("Cannot use both --keychain and --fido2.".to_string());
            } else if fido2 {
                init_with_fido2(&vault)?;
            } else if keychain {
                init_with_keychain(&vault)?;
            } else {
                let password = prompt_for_input("Enter password: ")?;
                let confirm = prompt_for_input("Confirm password: ")?;
                if password != confirm {
                    return Err("Passwords do not match".to_string());
                }

                match qpv2_core::utilities::password_checker(&password) {
                    Ok(strength) => println!("Password strength: {} bits", strength),
                    Err(e) => {
                        return Err(format!("Password validation failed: {}", e));
                    }
                }

                vault.generate_master_seed(AuthKey::Password(password), AuthMethod::Password)?;
            }
            println!("✓ Master seed generated successfully");
            println!(
                "⚠️  Make sure to backup your seed phrase using the 'mnemonic export' command"
            );
        }

        Commands::Mnemonic { command } => match command {
            MnemonicCommands::Import {
                variant,
                seed_file,
                keychain,
                fido2,
            } => {
                let variant = parse_variant(&variant)?;
                let vault = KeyVault::new(variant);

                let seed_phrase = if let Some(file_path) = seed_file {
                    SecureString::from_string(
                        fs::read_to_string(file_path).map_err(|e| e.to_string())?,
                    )
                } else {
                    prompt_for_input("Enter seed phrase: ")?
                };

                if keychain && fido2 {
                    return Err("Cannot use both --keychain and --fido2.".to_string());
                } else if fido2 {
                    import_with_fido2(&vault, seed_phrase)?;
                } else if keychain {
                    import_with_keychain(&vault, seed_phrase)?;
                } else {
                    let password = prompt_for_input("Enter password: ")?;
                    let confirm = prompt_for_input("Confirm password: ")?;
                    if password != confirm {
                        return Err("Passwords do not match".to_string());
                    }

                    match qpv2_core::utilities::password_checker(&password) {
                        Ok(strength) => println!("Password strength: {} bits", strength),
                        Err(e) => {
                            return Err(format!("Password validation failed: {}", e));
                        }
                    }

                    vault.import_seed_phrase(
                        seed_phrase,
                        AuthKey::Password(password),
                        AuthMethod::Password,
                    )?;
                }
                println!("✓ Seed phrase imported successfully");
            }

            MnemonicCommands::Export { output } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let auth = get_auth_key()?;
                let seed_phrase = vault.export_seed_phrase(auth)?;

                if let Some(output_path) = output {
                    fs::write(output_path, &*seed_phrase).map_err(|e| e.to_string())?;
                    println!("✓ Seed phrase exported to file");
                } else {
                    println!("Seed phrase:");
                    println!("{}", &*seed_phrase);
                }
            }
        },

        Commands::Account { command } => {
            match command {
                AccountCommands::New => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let auth = get_auth_key()?;
                    let lock_args = vault.gen_new_account(auth)?;
                    println!("✓ New account created");
                    println!("Identifier(CKB quantum lock script args): {}", lock_args);
                }

                AccountCommands::List => {
                    let accounts = KeyVault::get_all_sphincs_lock_args()?;
                    if accounts.is_empty() {
                        println!("No accounts found. Run `qpv2-cli account new` to generate a new SPHINCS+ account");
                    } else {
                        println!("Accounts ({}):", accounts.len());
                        println!("  Index  Account Identifier (CKB Quantum Lock Args)");
                        println!("  ─────────────────────────────────────────────────────────────────────");
                        for (idx, lock_args) in accounts.iter().enumerate() {
                            println!("  [{}]    {}", idx, lock_args);
                        }
                    }
                }

                AccountCommands::Recover { count } => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let auth = get_auth_key()?;
                    let accounts = vault.recover_accounts(auth, count)?;

                    println!("✓ Recovered {} accounts:", accounts.len());
                    for (idx, lock_args) in accounts.iter().enumerate() {
                        println!("  [{}] {}", idx, lock_args);
                    }
                }

                AccountCommands::TryGenBatch { start, count } => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let auth = get_auth_key()?;
                    let accounts = vault.try_gen_account_batch(auth, start, count)?;

                    println!("Generated {} accounts:", accounts.len());
                    for (idx, lock_args) in accounts.iter().enumerate() {
                        println!("  [{}] {}", start + idx as u32, lock_args);
                    }
                }
            }
        }

        Commands::Sign {
            identifier,
            message,
        } => {
            let variant = KeyVault::get_spx_variant()?;
            let vault = KeyVault::new(variant);

            let message_bytes = hex::decode(&message).map_err(|e| e.to_string())?;
            let auth = get_auth_key()?;

            let (signature, pub_key) = vault.raw_sign(auth, identifier, message_bytes)?;
            println!("Signature: {}", hex::encode(signature));
            println!("Public Key: {}", hex::encode(pub_key));
        }

        Commands::Verify {
            variant,
            public_key,
            message,
            signature,
        } => {
            let variant = parse_variant(&variant)?;
            let message_bytes = hex::decode(&message).map_err(|e| e.to_string())?;
            let public_key_bytes = hex::decode(&public_key).map_err(|e| e.to_string())?;
            let signature_bytes = hex::decode(&signature).map_err(|e| e.to_string())?;

            let vault = KeyVault::new(variant);

            let is_valid = vault.raw_verify(&public_key_bytes, &message_bytes, &signature_bytes)?;
            if is_valid {
                println!("✓ Signature is valid");
            } else {
                println!("✗ Signature is invalid");
            }
        }

        Commands::Ckb { command } => match command {
            CkbCommands::Sign { lock_args, message } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let message_bytes = hex::decode(&message).map_err(|e| e.to_string())?;
                let auth = get_auth_key()?;

                let signature = vault.ckb_sign(auth, lock_args, message_bytes)?;
                println!("Signature: {}", hex::encode(signature));
            }

            CkbCommands::GetTxMessage { tx_file } => {
                let tx_data = fs::read(tx_file).map_err(|e| e.to_string())?;
                let message = qpv2_core::utilities::get_ckb_tx_message_all(tx_data)?;
                println!("CKB Tx message hash: {}", hex::encode(message));
            }
        },

        Commands::Clear => {
            print!("Are you sure you want to clear all wallet data? (yes/no): ");
            io::stdout().flush().map_err(|e| e.to_string())?;

            let mut confirmation = String::new();
            io::stdin()
                .read_line(&mut confirmation)
                .map_err(|e| e.to_string())?;

            if confirmation.trim().to_lowercase() == "yes" {
                let _ = keychain::delete_key();
                KeyVault::clear_database()?;
                println!("✓ All wallet data cleared");
            } else {
                println!("Operation cancelled");
            }
        }

        Commands::Info => {
            let accounts = KeyVault::get_all_sphincs_lock_args()?;
            let data_path = qpv2_core::db::get_data_dir().map_err(|e| e.to_string())?;

            // Read wallet info to get authentication method
            let wallet_info = KeyVault::read_wallet_info()?;

            let (auth_method_display, compatible_frontends) = match wallet_info.auth_method {
                AuthMethod::Password => ("Password".to_string(), "CLI and GUI"),
                AuthMethod::Keychain => {
                    (keychain::keystore_display_name().to_string(), "CLI and GUI")
                }
                AuthMethod::Fido2 { .. } => ("FIDO2 Security Key".to_string(), "CLI and GUI"),
            };

            println!("\n╔════════════════════════════════════════════════════════════════╗");
            println!("║                     Wallet Information                         ║");
            println!("╚════════════════════════════════════════════════════════════════╝");
            println!();
            println!("  SPHINCS+ Variant      : {}", wallet_info.spx_variant);
            println!(
                "  Mnemonic Words        : {}",
                wallet_info.spx_variant.required_bip39_size_in_word_total()
            );
            println!("  Authentication Method : {}", auth_method_display);
            println!("  Compatible Frontends  : {}", compatible_frontends);
            println!("  Total Accounts        : {}", accounts.len());
            println!("  Data Storage Path     : {}", data_path.display());
            println!();
        }
    }

    Ok(())
}
