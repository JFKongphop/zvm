//! zvm: programmable money for shielded ZEC.
//!
//!   zvm init   --dir data --network 1 --admin <owner id>   writes data/config.json
//!   zvm wallet-init --wallet ~/.zvm/vault --network main   creates the vault wallet (keep it outside the repo)
//!   zvm wallet-info --wallet ~/.zvm/vault                  address and viewing-key fingerprint (never the seed)
//!   zvm serve  --dir data --wallet ~/.zvm/vault            runs the sequencer, the vault loop and the HTTP API
//!   zvm serve  --dir data --fake                           same, on a stand-in chain with no wallet
//!   zvm replay --dir data                                  rebuilds the state from data/log.jsonl and checks every root
//!   zvm owner  <ed25519 pubkey hex>                         prints the owner id for a key
//!   zvm keygen --out key.txt                               a signing key for testing from the terminal
//!   zvm submit --key key.txt --cmd '{"op":"transfer",...}' signs a command and sends it to a running node

use zvm_node::{api, cmd, seq, vault};

fn network_id(name: &str) -> u64 {
    if name == "main" { 1 } else { 2 }
}

use clap::{Parser, Subcommand};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 1)]
        network: u64,
        /// owner id (0x + 40 hex) allowed to set vault-wide rules
        #[arg(long)]
        admin: String,
    },
    Serve {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 8790)]
        port: u16,
        /// stand-in chain: wall-clock time, deposits through /api/dev/deposit
        #[arg(long)]
        fake: bool,
        /// the vault wallet directory (from `wallet-init`)
        #[arg(long)]
        wallet: Option<PathBuf>,
        /// show the full viewing key in /api/info, so anyone can audit every deposit and payout
        #[arg(long)]
        publish_viewing_key: bool,
        /// the web app's folder
        #[arg(long, default_value = "web")]
        web: PathBuf,
        /// address to listen on (0.0.0.0 to serve beyond this machine)
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },
    WalletInit {
        #[arg(long)]
        wallet: PathBuf,
        /// `main` or `test`
        #[arg(long)]
        network: String,
    },
    WalletInfo {
        #[arg(long)]
        wallet: PathBuf,
    },
    Replay {
        #[arg(long)]
        dir: PathBuf,
    },
    Owner {
        pubkey: String,
    },
    Keygen {
        #[arg(long)]
        out: PathBuf,
    },
    Submit {
        #[arg(long)]
        key: PathBuf,
        /// the command as JSON, e.g. {"op":"transfer","to":"0x…","tokens":["0x…"],"amounts":[1000]}
        #[arg(long)]
        cmd: String,
        #[arg(long, default_value = "http://127.0.0.1:8790")]
        node: String,
    },
}

fn http_get(url: &str) -> anyhow::Result<serde_json::Value> {
    let out = std::process::Command::new("curl").args(["-sf", url]).output()?;
    anyhow::ensure!(out.status.success(), "GET {url} failed");
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn http_post(url: &str, body: &str) -> anyhow::Result<String> {
    let out = std::process::Command::new("curl").args(["-s", "-H", "content-type: application/json", "-d", body, url]).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn config(dir: &std::path::Path) -> anyhow::Result<seq::Config> {
    let p = dir.join("config.json");
    Ok(serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?)?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // both TLS backends end up in the dependency tree, so one has to be chosen explicitly
    let _ = rustls::crypto::ring::default_provider().install_default();
    match Cli::parse().command {
        Command::Init { dir, network, admin } => {
            cmd::parse_addr(&admin).map_err(anyhow::Error::msg)?;
            std::fs::create_dir_all(&dir)?;
            let p = dir.join("config.json");
            anyhow::ensure!(!p.exists(), "{} already exists", p.display());
            // the vault id stands in for moneyvm's token contract address in commitments
            let seed: [u8; 32] = <sha3::Sha3_256 as sha3::Digest>::digest(format!("zvm-host:{network}:{admin}")).into();
            let host = cmd::owner_of(&seed);
            let cfg = seq::Config { network_id: network, host: cmd::hex_addr(&host), admin };
            std::fs::write(&p, serde_json::to_string_pretty(&cfg)?)?;
            println!("{}", serde_json::to_string_pretty(&cfg)?);
        }
        Command::Serve { dir, port, fake, wallet, publish_viewing_key, web, bind } => {
            let cfg = config(&dir)?;
            let seq = seq::Sequencer::open(cfg.clone(), Some(dir.join("log.jsonl")))?;
            println!("replayed {} entries, root {}", seq.log.len(), seq.state.root());
            let book_path = dir.join("vault.json");
            let book = vault::Book::load(&book_path);
            let (tip, vault_info) = match (fake, &wallet) {
                (true, None) => (api::Tip::Fake, None),
                (false, Some(w)) => {
                    let wallet = zvm_wallet::Wallet::open(w)?;
                    let info = wallet.info()?;
                    anyhow::ensure!(network_id(&info.network) == cfg.network_id, "the wallet is on {} but config.json says network {}", info.network, cfg.network_id);
                    println!("vault {} (viewing key fingerprint {})", info.address, info.viewing_key_fingerprint);
                    let v = api::VaultInfo {
                        address: info.address,
                        viewing_key_fingerprint: info.viewing_key_fingerprint,
                        viewing_key: publish_viewing_key.then_some(info.viewing_key),
                        wallet,
                    };
                    (api::Tip::Chain(zvm_core::Env::default()), Some(v))
                }
                _ => anyhow::bail!("pass exactly one of --fake or --wallet <dir>"),
            };
            let node = Arc::new(tokio::sync::Mutex::new(api::Node { seq, tip, vault: vault_info, book }));
            if let Some(w) = wallet {
                vault::Vault { wallet_dir: w, book_path }.spawn(node.clone());
            }
            let app = api::app(node, &web);
            let listener = tokio::net::TcpListener::bind((bind.as_str(), port)).await?;
            println!("zvm on http://{bind}:{port}");
            axum::serve(listener, app).await?;
        }
        Command::Replay { dir } => {
            let cfg = config(&dir)?;
            let log = seq::read_log(&dir.join("log.jsonl"))?;
            match seq::replay(&cfg, &log) {
                Ok(s) => println!("ok: {} entries replayed, every root matches, final root {}", log.len(), s.root()),
                Err((i, e)) => anyhow::bail!("entry {i}: {e}"),
            }
        }
        Command::WalletInit { wallet, network } => {
            let w = zvm_wallet::Wallet::init(&wallet, &network, None).await?;
            let info = w.info()?;
            println!("vault wallet created in {} (recovery phrase in seed.txt there, never printed)", wallet.display());
            println!("address {}\nviewing key fingerprint {}", info.address, info.viewing_key_fingerprint);
        }
        Command::WalletInfo { wallet } => {
            let info = zvm_wallet::Wallet::open(&wallet)?.info()?;
            println!("network {}\naddress {}\nviewing key fingerprint {}\ncan spend {}", info.network, info.address, info.viewing_key_fingerprint, info.can_spend);
        }
        Command::Keygen { out } => {
            use std::io::Write;
            let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&out)?;
            writeln!(f, "{}", hex::encode(key.to_bytes()))?;
            let pk = key.verifying_key().to_bytes();
            println!("public key {}\nowner id   {}", hex::encode(pk), cmd::hex_addr(&cmd::owner_of(&pk)));
        }
        Command::Submit { key, cmd: c, node } => {
            use ed25519_dalek::Signer;
            let secret: [u8; 32] = hex::decode(std::fs::read_to_string(&key)?.trim())?.try_into().map_err(|_| anyhow::anyhow!("bad key file"))?;
            let key = ed25519_dalek::SigningKey::from_bytes(&secret);
            let pk = key.verifying_key().to_bytes();
            let owner = cmd::hex_addr(&cmd::owner_of(&pk));
            let info: serde_json::Value = http_get(&format!("{node}/api/info"))?;
            let me: serde_json::Value = http_get(&format!("{node}/api/owner/{owner}"))?;
            let cmd_json: serde_json::Value = serde_json::from_str(&c)?;
            let payload = serde_json::json!({ "net": info["network_id"], "nonce": me["nonce"].as_u64().unwrap_or(0) + 1, "signer": hex::encode(pk), "cmd": cmd_json }).to_string();
            let sig = hex::encode(key.sign(payload.as_bytes()).to_bytes());
            println!("{}", http_post(&format!("{node}/api/cmd"), &serde_json::json!({ "payload": payload, "sig": sig }).to_string())?);
        }
        Command::Owner { pubkey } => {
            let pk: [u8; 32] = hex::decode(pubkey.trim_start_matches("0x"))?.try_into().map_err(|_| anyhow::anyhow!("32 bytes"))?;
            println!("{}", cmd::hex_addr(&cmd::owner_of(&pk)));
        }
    }
    Ok(())
}
