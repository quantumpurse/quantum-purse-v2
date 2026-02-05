use clap::{Parser, Subcommand};
use quantum_purse_key_vault::{types::SpxVariant, KeyVault, Util};
use rpassword::read_password;
use std::fs;
use std::io::{self, Write};
use zeroize::Zeroize;

#[derive(Parser)]
#[command(name = "qpkv")]
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
        /// Account identifier (CKB quantum lock args). Run `qpkv account list` to see all accounts
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

fn promt_for_input(prompt: &str) -> Result<String, String> {
    print!("{}", prompt);
    io::stdout().flush().map_err(|e| e.to_string())?;
    let input = read_password().map_err(|e| e.to_string())?;
    Ok(input)
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

            let mut password = promt_for_input("Enter password: ")?.into_bytes();

            let mut confirm = promt_for_input("Confirm password: ")?.into_bytes();
            if password != confirm {
                password.zeroize();
                confirm.zeroize();
                return Err("Passwords do not match".to_string());
            }
            confirm.zeroize();

            match Util::password_checker(password.clone()) {
                Ok(strength) => println!("Password strength: {} bits", strength),
                Err(e) => {
                    password.zeroize();
                    confirm.zeroize();
                    return Err(format!("Password validation failed: {}", e));
                }
            }

            vault.generate_master_seed(password)?;
            println!("✓ Master seed generated successfully");
            println!(
                "⚠️  Make sure to backup your seed phrase using the 'mnemonic export' command"
            );
        }

        Commands::Mnemonic { command } => match command {
            MnemonicCommands::Import { variant, seed_file } => {
                let variant = parse_variant(&variant)?;
                let vault = KeyVault::new(variant);

                let mut seed_phrase = if let Some(file_path) = seed_file {
                    fs::read_to_string(file_path).map_err(|e| e.to_string())?
                } else {
                    promt_for_input("Enter seed phrase: ")?
                };

                let mut password = promt_for_input("Enter password: ")?.into_bytes();

                let mut confirm = promt_for_input("Confirm password: ")?.into_bytes();
                if password != confirm {
                    password.zeroize();
                    confirm.zeroize();
                    seed_phrase.zeroize();
                    return Err("Passwords do not match".to_string());
                }
                confirm.zeroize();

                match Util::password_checker(password.clone()) {
                    Ok(strength) => println!("Password strength: {} bits", strength),
                    Err(e) => {
                        password.zeroize();
                        confirm.zeroize();
                        seed_phrase.zeroize();
                        return Err(format!("Password validation failed: {}", e));
                    }
                }

                vault.import_seed_phrase(seed_phrase.into_bytes(), password)?;
                println!("✓ Seed phrase imported successfully");
            }

            MnemonicCommands::Export { output } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let password = promt_for_input("Enter password: ")?.into_bytes();
                let seed_phrase = vault.export_seed_phrase(password)?;
                let mut seed_str = String::from_utf8(seed_phrase).map_err(|e| e.to_string())?;

                if let Some(output_path) = output {
                    fs::write(output_path, &seed_str).map_err(|e| e.to_string())?;
                    println!("✓ Seed phrase exported to file");
                } else {
                    println!("Seed phrase:");
                    println!("{}", seed_str);
                }
                seed_str.zeroize();
            }
        },

        Commands::Account { command } => {
            match command {
                AccountCommands::New => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let password = promt_for_input("Enter password: ")?.into_bytes();
                    let lock_args = vault.gen_new_account(password)?;
                    println!("✓ New account created");
                    println!("Identifier(CKB quantum lock script args): {}", lock_args);
                }

                AccountCommands::List => {
                    let accounts = KeyVault::get_all_sphincs_lock_args()?;
                    if accounts.is_empty() {
                        println!("No accounts found. Run `qpkv account new` to generate a new SPHINCS+ account");
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

                    let password = promt_for_input("Enter password: ")?.into_bytes();
                    let accounts = vault.recover_accounts(password, count)?;

                    println!("✓ Recovered {} accounts:", accounts.len());
                    for (idx, lock_args) in accounts.iter().enumerate() {
                        println!("  [{}] {}", idx, lock_args);
                    }
                }

                AccountCommands::TryGenBatch { start, count } => {
                    let variant = KeyVault::get_spx_variant()?;
                    let vault = KeyVault::new(variant);

                    let password = promt_for_input("Enter password: ")?.into_bytes();
                    let accounts = vault.try_gen_account_batch(password, start, count)?;

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
            let password = promt_for_input("Enter password: ")?.into_bytes();

            let (signature, pub_key) = vault.raw_sign(password, identifier, message_bytes)?;
            println!("Signature: {}", hex::encode(signature));
            println!("Public Key: {}", hex::encode(pub_key));
        }

        Commands::Ckb { command } => match command {
            CkbCommands::Sign { lock_args, message } => {
                let variant = KeyVault::get_spx_variant()?;
                let vault = KeyVault::new(variant);

                let message_bytes = hex::decode(&message).map_err(|e| e.to_string())?;
                let password = promt_for_input("Enter password: ")?.into_bytes();

                let signature = vault.ckb_sign(password, lock_args, message_bytes)?;
                println!("Signature: {}", hex::encode(signature));
            }

            CkbCommands::GetTxMessage { tx_file } => {
                let tx_data = fs::read(tx_file).map_err(|e| e.to_string())?;
                let message = Util::get_ckb_tx_message_all(tx_data)?;
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
            let data_path =
                quantum_purse_key_vault::db::get_data_dir().map_err(|e| e.to_string())?;

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
