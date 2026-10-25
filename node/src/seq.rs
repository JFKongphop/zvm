//! The sequencer: orders deposits and signed commands, applies them, and keeps the public log.
//!
//! The log is the whole truth. Each line is one accepted entry with the Zcash block time and height it ran at and the
//! state root after it, so `replay` can rebuild the state from nothing and check every root along the way.
//! Refused commands change nothing and are not logged.

use crate::cmd::{self, hex_addr, hex_word};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use zvm_core::{Addr, Env, Machine, codec};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub network_id: u64,
    pub host: String,
    pub admin: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    /// Shielded ZEC that arrived at the vault (`txid` lets anyone with the viewing key check it).
    Deposit { txid: String, owner: String, zats: u64, #[serde(default)] bound: u64, #[serde(default)] time_mode: u8, #[serde(default)] payload: u32 },
    Command { payload: String, sig: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub index: u64,
    pub time: u64,
    pub height: u64,
    #[serde(flatten)]
    pub body: Body,
    pub result: Value,
    pub root: String,
}

#[derive(Clone)]
pub struct State {
    pub machine: Machine,
    pub nonces: BTreeMap<Addr, u64>,
}

impl State {
    pub fn new(cfg: &Config) -> Result<Self, String> {
        Ok(State {
            machine: Machine::new(cfg.network_id, cmd::parse_addr(&cfg.host)?, cmd::parse_addr(&cfg.admin)?),
            nonces: BTreeMap::new(),
        })
    }

    /// The VM's state root, extended with every signer's nonce.
    pub fn root(&self) -> String {
        let mut h = Sha3_256::new();
        h.update(self.machine.state_root());
        for (a, n) in &self.nonces {
            h.update(a);
            h.update(n.to_be_bytes());
        }
        format!("0x{}", hex::encode(h.finalize()))
    }

    /// Applies one entry body. Deterministic: the same state, env and body always give the same result.
    pub fn apply(&mut self, env: Env, body: &Body) -> Result<Value, String> {
        match body {
            Body::Deposit { owner, zats, bound, time_mode, payload, .. } => {
                let t = self
                    .machine
                    .deposit(env, cmd::parse_addr(owner)?, *zats as u128, *bound, *time_mode, *payload)
                    .map_err(|e| e.to_string())?;
                Ok(json!({ "token": hex_word(t) }))
            }
            Body::Command { payload, sig } => {
                let (signed, caller) = cmd::verify(payload, sig)?;
                if signed.net != self.machine.network_id {
                    return Err(format!("signed for network {}, this vault is {}", signed.net, self.machine.network_id));
                }
                let expected = self.nonces.get(&caller).copied().unwrap_or(0) + 1;
                if signed.nonce != expected {
                    return Err(format!("nonce must be {expected}"));
                }
                let result = cmd::run(&mut self.machine, env, caller, &signed.cmd)?;
                self.nonces.insert(caller, expected);
                Ok(result)
            }
        }
    }
}

pub struct Sequencer {
    pub cfg: Config,
    pub state: State,
    pub log: Vec<Entry>,
    file: Option<PathBuf>,
}

impl Sequencer {
    /// Opens a log file (creating it if missing) and replays it.
    pub fn open(cfg: Config, file: Option<PathBuf>) -> anyhow::Result<Self> {
        let mut log = vec![];
        if let Some(p) = &file {
            if p.exists() {
                log = read_log(p)?;
            }
        }
        let state = replay(&cfg, &log).map_err(|(i, e)| anyhow::anyhow!("log entry {i} does not replay: {e}"))?;
        Ok(Sequencer { cfg, state, log, file })
    }

    fn append(&mut self, env: Env, body: Body) -> Result<Entry, String> {
        let result = self.state.apply(env, &body)?;
        let entry = Entry { index: self.log.len() as u64, time: env.time, height: env.height, body, result, root: self.state.root() };
        if let Some(p) = &self.file {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| e.to_string())?;
            writeln!(f, "{}", serde_json::to_string(&entry).unwrap()).map_err(|e| e.to_string())?;
        }
        self.log.push(entry.clone());
        Ok(entry)
    }

    pub fn command(&mut self, env: Env, payload: String, sig: String) -> Result<Entry, String> {
        self.append(env, Body::Command { payload, sig })
    }

    pub fn deposit(&mut self, env: Env, txid: String, owner: Addr, zats: u64, bound: u64, time_mode: u8, payload: u32) -> Result<Entry, String> {
        if self.log.iter().any(|e| matches!(&e.body, Body::Deposit { txid: t, .. } if *t == txid)) {
            return Err(format!("deposit {txid} is already in the log"));
        }
        self.append(env, Body::Deposit { txid, owner: hex_addr(&owner), zats, bound, time_mode, payload })
    }

    /// What an owner holds, with each token's fields and the rule attached to it.
    pub fn holdings(&self, owner: &Addr) -> Value {
        let m = &self.state.machine;
        let tokens: Vec<Value> = m
            .holdings(owner)
            .into_iter()
            .map(|(t, bal)| {
                let c = codec::deserialize(t);
                let inst = m.instructions.get(&codec::cid(t)).copied().unwrap_or_default();
                let rule = if inst.is_zero() {
                    Value::Null
                } else {
                    let kind = match codec::rd(inst) {
                        1 => "escrow",
                        2 => "htlc",
                        0 => "rule",
                        _ => "extension",
                    };
                    json!({ "kind": kind, "inst": hex_word(inst), "commitment": hex_word(codec::imm_1(inst)), "expiry": codec::imm_2(inst) })
                };
                json!({
                    "token": hex_word(t), "zats": bal as u64,
                    "id": c.id, "seq": c.seq, "slc": c.slc, "iat": c.iat, "ts": c.ts,
                    "time_mode": if c.tmd == 1 { "unlock" } else { "expiry" }, "payload": c.pl,
                    "rule": rule, "lineage_rules": m.bop_sequences.contains_key(&c.id),
                    "lineage_seq": m.bop_sequences.get(&c.id).map(|s| s.iter().filter(|w| !w.is_zero()).map(|w| hex_word(*w)).collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect();
        json!({
            "owner": hex_addr(owner),
            "nonce": self.state.nonces.get(owner).copied().unwrap_or(0),
            "zats": tokens.iter().map(|t| t["zats"].as_u64().unwrap_or(0)).sum::<u64>(),
            "tokens": tokens,
        })
    }
}

pub fn read_log(p: &Path) -> anyhow::Result<Vec<Entry>> {
    let f = std::io::BufReader::new(std::fs::File::open(p)?);
    let mut out = vec![];
    for line in f.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            out.push(serde_json::from_str(&line)?);
        }
    }
    Ok(out)
}

/// Rebuilds the state from a log and checks every entry's index, result and root.
pub fn replay(cfg: &Config, log: &[Entry]) -> Result<State, (u64, String)> {
    let mut s = State::new(cfg).map_err(|e| (0, e))?;
    for (i, e) in log.iter().enumerate() {
        let fail = |m: String| (e.index, m);
        if e.index != i as u64 {
            return Err(fail(format!("expected index {i}")));
        }
        let env = Env { time: e.time, height: e.height, network_id: cfg.network_id };
        let result = s.apply(env, &e.body).map_err(|m| fail(format!("refused on replay: {m}")))?;
        if result != e.result {
            return Err(fail("result differs".into()));
        }
        if s.root() != e.root {
            return Err(fail(format!("root differs: log {} replay {}", e.root, s.root())));
        }
    }
    Ok(s)
}
