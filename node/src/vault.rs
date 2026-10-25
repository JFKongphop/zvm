//! The vault loop: connects the sequencer to Zcash.
//!
//! Every tick it syncs the wallet, moves the VM clock to the newest block, credits confirmed deposits whose memo names
//! an owner, pays queued withdrawals, and now and then writes the state root into a memo to itself.
//!
//! Memos:
//!   deposit   `zvm1:dep:<owner id>`  or  `zvm1:dep:<owner id>:<bound>:<time mode>:<payload>`
//!   payout    `zvm1:out:<log index>`
//!   anchor    `zvm1:root:<log index>:<state root>`

use crate::api::{Shared, Tip};
use crate::cmd::parse_addr;
use crate::seq::Body;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zvm_core::{Addr, Env};
use zvm_wallet::{Outgoing, Wallet};

/// Taken from each withdrawal to pay the Zcash fee (ZIP-317: 5,000 zats per logical action).
pub const WITHDRAW_FEE: u64 = 15_000;
/// Blocks a deposit must have before it is credited (the block it is in counts as one).
pub const CONFIRMATIONS: u32 = 2;
const ANCHOR_EVERY: Duration = Duration::from_secs(30 * 60);
const ANCHOR_ZATS: u64 = 1_000;
const TICK: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositMemo {
    pub owner: Addr,
    pub bound: u64,
    pub time_mode: u8,
    pub payload: u32,
}

pub fn parse_deposit_memo(memo: &str) -> Option<DepositMemo> {
    let rest = memo.trim().strip_prefix("zvm1:dep:")?;
    let parts: Vec<&str> = rest.split(':').collect();
    let owner = parse_addr(parts[0]).ok()?;
    match parts.len() {
        1 => Some(DepositMemo { owner, bound: 0, time_mode: 0, payload: 0 }),
        4 => Some(DepositMemo {
            owner,
            bound: parts[1].parse().ok()?,
            time_mode: parts[2].parse().ok().filter(|m| *m <= 1)?,
            payload: parts[3].parse().ok()?,
        }),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Payout {
    pub to: String,
    pub zats: u64,
    pub txid: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Anchor {
    pub index: u64,
    pub root: String,
    pub txid: String,
    pub at: u64,
}

/// What the vault has done on chain, kept next to the log.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Book {
    /// log index of a withdrawal → its payout
    pub payouts: BTreeMap<u64, Payout>,
    pub anchors: Vec<Anchor>,
    /// payments that reached the vault without a valid deposit memo (not credited; shown so they can be refunded)
    #[serde(default)]
    pub unclaimed: BTreeMap<String, u64>,
}

impl Book {
    pub fn load(p: &Path) -> Self {
        std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    fn save(&self, p: &Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}

pub struct Vault {
    pub wallet_dir: PathBuf,
    pub book_path: PathBuf,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

impl Vault {
    /// Runs forever on its own thread: wallet futures hold the sqlite connection across awaits and are not `Send`.
    pub fn spawn(self, node: Shared) {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
            rt.block_on(async move {
                loop {
                    if let Err(e) = self.tick(&node).await {
                        eprintln!("[vault] {e:#}");
                    }
                    tokio::time::sleep(TICK).await;
                }
            });
        });
    }

    pub async fn tick(&self, node: &Shared) -> anyhow::Result<()> {
        let wallet = Wallet::open(&self.wallet_dir)?;
        let tip = wallet.sync().await?;
        let Some((height, time)) = wallet.tip()? else { return Ok(()) };
        let received = wallet.received()?;

        // 1. clock + deposits, under the lock (no network calls here)
        let (to_pay, anchor_due) = {
            let mut n = node.lock().await;
            let net = n.seq.cfg.network_id;
            n.tip = Tip::Chain(Env { time, height: height as u64, network_id: net });

            let mut book = n.book.clone();
            for r in &received {
                let Some(mined) = r.mined_height else { continue };
                if tip + 1 < mined + CONFIRMATIONS {
                    continue;
                }
                let logged = n.seq.log.iter().any(|e| matches!(&e.body, Body::Deposit { txid, .. } if *txid == r.txid));
                if logged || book.unclaimed.contains_key(&r.txid) {
                    continue;
                }
                match r.memo.as_deref().and_then(parse_deposit_memo) {
                    Some(d) => {
                        let env = Env { time: r.block_time.unwrap_or(time), height: mined as u64, network_id: net };
                        match n.seq.deposit(env, r.txid.clone(), d.owner, r.zats, d.bound, d.time_mode, d.payload) {
                            Ok(e) => println!("[vault] credited {} zats from {} as entry {}", r.zats, r.txid, e.index),
                            Err(e) => {
                                eprintln!("[vault] deposit {} refused: {e}", r.txid);
                                book.unclaimed.insert(r.txid.clone(), r.zats);
                            }
                        }
                    }
                    None => {
                        book.unclaimed.insert(r.txid.clone(), r.zats);
                    }
                }
            }

            let to_pay: Vec<(u64, Payout)> = n
                .seq
                .log
                .iter()
                .filter(|e| !book.payouts.get(&e.index).is_some_and(|p| p.txid.is_some()))
                .filter_map(|e| {
                    let p = e.result.get("payout")?;
                    Some((e.index, Payout { to: p["to"].as_str()?.to_string(), zats: p["zats"].as_u64()?, ..Default::default() }))
                })
                .collect();

            let last = book.anchors.last();
            let entries = n.seq.log.len() as u64;
            let anchor_due = entries > 0
                && last.is_none_or(|a| a.index + 1 < entries && now().saturating_sub(a.at) >= ANCHOR_EVERY.as_secs());
            let anchor_due = anchor_due.then(|| (entries - 1, n.seq.log.last().unwrap().root.clone()));

            n.book = book;
            n.book.save(&self.book_path);
            (to_pay, anchor_due)
        };

        // 2. payouts, one transaction each, outside the lock (proving takes seconds)
        for (index, mut p) in to_pay {
            let net_zats = p.zats.saturating_sub(WITHDRAW_FEE);
            let result = if net_zats == 0 {
                Err(anyhow::anyhow!("withdrawal is not above the {WITHDRAW_FEE} zat fee"))
            } else {
                wallet.send(&[Outgoing { to: p.to.clone(), zats: net_zats, memo: Some(format!("zvm1:out:{index}")) }]).await
            };
            match result {
                Ok(txid) => {
                    println!("[vault] paid entry {index}: {net_zats} zats to {} in {txid}", p.to);
                    p.txid = Some(txid);
                    p.error = None;
                }
                Err(e) => p.error = Some(format!("{e:#}")), // retried next tick (often: change not confirmed yet)
            }
            let mut n = node.lock().await;
            n.book.payouts.insert(index, p);
            n.book.save(&self.book_path);
            if n.book.payouts[&index].txid.is_none() {
                break; // wait for confirmations before trying the next one
            }
        }

        // 3. anchor the state root
        if let Some((index, root)) = anchor_due {
            let me = wallet.info()?.address;
            match wallet.send(&[Outgoing { to: me, zats: ANCHOR_ZATS, memo: Some(format!("zvm1:root:{index}:{root}")) }]).await {
                Ok(txid) => {
                    println!("[vault] anchored entry {index} root in {txid}");
                    let mut n = node.lock().await;
                    n.book.anchors.push(Anchor { index, root, txid, at: now() });
                    n.book.save(&self.book_path);
                }
                Err(e) => eprintln!("[vault] anchor failed (retrying later): {e:#}"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_memos() {
        let id = format!("0x{}", "a1".repeat(20));
        assert_eq!(parse_deposit_memo(&format!("zvm1:dep:{id}")).unwrap().owner, [0xA1; 20]);
        let d = parse_deposit_memo(&format!("zvm1:dep:{id}:1800000000:1:7")).unwrap();
        assert_eq!((d.bound, d.time_mode, d.payload), (1_800_000_000, 1, 7));
        assert!(parse_deposit_memo(&format!("zvm1:dep:{id}:1:9:0")).is_none(), "time mode is 0 or 1");
        assert!(parse_deposit_memo("zvm1:dep:0x1234").is_none());
        assert!(parse_deposit_memo("hello").is_none());
    }
}
