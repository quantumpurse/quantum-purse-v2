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
        /// Account identifier (CKB quantum lock args). Run `qpv2 account list` to see all accounts
        #[arg(short, long)]
        identifier: String,
        /// Message to sign (hex-encoded)
        #[arg(short, long)]
        message: String,
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

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { variant } => {
            let variant = parse_variant(&variant)?;
            let vault = KeyVault::new(variant);

            println!("Initializing wallet with variant: {}", variant);
            println!(
                "Required mnemonic words: {}",
                variant.required_bip39_size_in_word_total()
            );

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
            println!("✓ Master seed generated successfully");
            println!(
                "⚠️  Make sure to backup your seed phrase using the 'mnemonic export' command"
            );
        }

        Commands::Mnemonic { command } => match command {
            MnemonicCommands::Import { variant, seed_file } => {
                let variant = parse_variant(&variant)?;
                let vault = KeyVault::new(variant);

                let seed_phrase = if let Some(file_path) = seed_file {
                    SecureString::from_string(
                        fs::read_to_string(file_path).map_err(|e| e.to_string())?,
                    )
                } else {
                    prompt_for_input("Enter seed phrase: ")?
                };

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
                println!("✓ Seed phrase imported successfully");
            }

            MnemonicCommands::Export { output } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let password = prompt_for_input("Enter password: ")?;
                let seed_phrase = vault.export_seed_phrase(AuthKey::Password(password))?;

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

                    let password = prompt_for_input("Enter password: ")?;
                    let lock_args = vault.gen_new_account(AuthKey::Password(password))?;
                    println!("✓ New account created");
                    println!("Identifier(CKB quantum lock script args): {}", lock_args);
                }

                AccountCommands::List => {
                    let accounts = KeyVault::get_all_sphincs_lock_args()?;
                    if accounts.is_empty() {
                        println!("No accounts found. Run `qpv2 account new` to generate a new SPHINCS+ account");
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

                    let password = prompt_for_input("Enter password: ")?;
                    let accounts = vault.recover_accounts(AuthKey::Password(password), count)?;

                    println!("✓ Recovered {} accounts:", accounts.len());
                    for (idx, lock_args) in accounts.iter().enumerate() {
                        println!("  [{}] {}", idx, lock_args);
                    }
                }

                AccountCommands::TryGenBatch { start, count } => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let password = prompt_for_input("Enter password: ")?;
                    let accounts =
                        vault.try_gen_account_batch(AuthKey::Password(password), start, count)?;

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
            let password = prompt_for_input("Enter password: ")?;

            let (signature, pub_key) =
                vault.raw_sign(AuthKey::Password(password), identifier, message_bytes)?;
            println!("Signature: {}", hex::encode(signature));
            println!("Public Key: {}", hex::encode(pub_key));
        }

        Commands::Ckb { command } => match command {
            CkbCommands::Sign { lock_args, message } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let message_bytes = hex::decode(&message).map_err(|e| e.to_string())?;
                let password = prompt_for_input("Enter password: ")?;

                let signature =
                    vault.ckb_sign(AuthKey::Password(password), lock_args, message_bytes)?;
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
                KeyVault::clear_database()?;
                println!("✓ All wallet data cleared");
            } else {
                println!("Operation cancelled");
            }
        }

        Commands::Info => {
            let variant = KeyVault::get_spx_variant()?;
            let accounts = KeyVault::get_all_sphincs_lock_args()?;
            let data_path = qpv2_core::db::get_data_dir().map_err(|e| e.to_string())?;

            println!("\n╔════════════════════════════════════════════════════════════════╗");
            println!("║                     Wallet Information                         ║");
            println!("╚════════════════════════════════════════════════════════════════╝");
            println!();
            println!("  SPHINCS+ Variant      : {}", variant);
            println!(
                "  Security Level        : {} bits",
                variant.required_entropy_size_component() * 8
            );
            println!(
                "  Mnemonic Words        : {}",
                variant.required_bip39_size_in_word_total()
            );
            println!("  Total Accounts        : {}", accounts.len());
            println!("  Data Storage Path     : {}", data_path.display());
            println!();
        }
    }

    Ok(())
}
