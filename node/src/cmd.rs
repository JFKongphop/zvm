//! Commands users sign, and how they map onto the VM.
//!
//! A client sends `{ payload, sig }`. `payload` is the exact JSON text that was signed (so no canonical encoding is
//! needed on either side): `{ "net": 1, "nonce": 3, "signer": "<ed25519 pubkey hex>", "cmd": { "op": ..., ... } }`.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha3::{Digest, Sha3_256};
use zvm_core::{Addr, Env, Machine, U256, codec};

/// Owner id: the first 20 bytes of SHA3-256("zvm-owner-v1" || ed25519 public key).
pub fn owner_of(pubkey: &[u8; 32]) -> Addr {
    let mut h = Sha3_256::new();
    h.update(b"zvm-owner-v1");
    h.update(pubkey);
    h.finalize()[..20].try_into().unwrap()
}

pub fn hex_addr(a: &Addr) -> String {
    format!("0x{}", hex::encode(a))
}

pub fn hex_word(x: U256) -> String {
    format!("0x{}", hex::encode(x.to_be_bytes::<32>()))
}

fn strip(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

pub fn parse_addr(s: &str) -> Result<Addr, String> {
    let b = hex::decode(strip(s)).map_err(|_| format!("bad id {s}"))?;
    b.try_into().map_err(|_| format!("an id is 20 bytes: {s}"))
}

pub fn parse_word(s: &str) -> Result<U256, String> {
    U256::from_str_radix(strip(s), 16).map_err(|_| format!("bad 256-bit value {s}"))
}

fn parse_b32(s: &str) -> Result<[u8; 32], String> {
    let b = hex::decode(strip(s)).map_err(|_| format!("bad hex {s}"))?;
    b.try_into().map_err(|_| format!("expected 32 bytes: {s}"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    Transfer { to: String, tokens: Vec<String>, amounts: Vec<u64> },
    /// Leave the VM: the vault pays `amounts` in shielded ZEC to the Zcash address `to`.
    Withdraw { tokens: Vec<String>, amounts: Vec<u64>, to: String },
    Consolidate { tokens: Vec<String>, amounts: Vec<u64>, #[serde(default)] bound: u64, #[serde(default)] time_mode: u8, #[serde(default)] payload: u32 },
    Attach { token: String, inst: String },
    SetBaseSequence { id: u64, seq: Vec<String> },
    SetSequence { id: u8, seq: Vec<String> },
    SetGlobal { inst: String },
    EscrowLock { token: String, recipient: String, arbiter: String, salt: String, expiry: u64, #[serde(default)] mode: u8 },
    EscrowRelease { commitment: String, token: String },
    EscrowRefund { commitment: String, token: String },
    HtlcLock { token: String, commitment: String, expiry: u64, #[serde(default)] mode: u8 },
    HtlcClaim { sender: String, token: String, preimage: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signed {
    pub net: u64,
    pub nonce: u64,
    pub signer: String,
    pub cmd: Command,
}

/// Checks the signature over the exact payload bytes and parses it.
pub fn verify(payload: &str, sig: &str) -> Result<(Signed, Addr), String> {
    let signed: Signed = serde_json::from_str(payload).map_err(|e| format!("bad payload: {e}"))?;
    let pk = parse_b32(&signed.signer)?;
    let key = VerifyingKey::from_bytes(&pk).map_err(|_| "bad signer key".to_string())?;
    let sig: [u8; 64] = hex::decode(strip(sig)).ok().and_then(|b| b.try_into().ok()).ok_or("signature must be 64 bytes")?;
    key.verify(payload.as_bytes(), &Signature::from_bytes(&sig)).map_err(|_| "signature does not match".to_string())?;
    Ok((signed, owner_of(&pk)))
}

fn words(xs: &[String]) -> Result<Vec<U256>, String> {
    xs.iter().map(|s| parse_word(s)).collect()
}

fn amounts(xs: &[u64]) -> Vec<u128> {
    xs.iter().map(|&a| a as u128).collect()
}

fn sequence(xs: &[String]) -> Result<zvm_core::vm::Sequence, String> {
    if xs.len() > zvm_core::vm::MAX_SEQUENCE_SIZE {
        return Err("a sequence has at most 6 steps".into());
    }
    let mut s: zvm_core::vm::Sequence = Default::default();
    for (i, w) in words(xs)?.into_iter().enumerate() {
        s[i] = w;
    }
    Ok(s)
}

/// Runs one command for `caller`. Returns what the client needs to know (new tokens, commitments, payouts).
pub fn run(m: &mut Machine, env: Env, caller: Addr, cmd: &Command) -> Result<Value, String> {
    let e = |err: zvm_core::Error| err.to_string();
    Ok(match cmd {
        Command::Transfer { to, tokens, amounts: a } => {
            let moved = m.transfer(env, caller, parse_addr(to)?, &words(tokens)?, &amounts(a)).map_err(e)?;
            json!({ "tokens": moved.into_iter().map(hex_word).collect::<Vec<_>>() })
        }
        Command::Withdraw { tokens, amounts: a, to } => {
            if to.trim().is_empty() {
                return Err("withdraw needs a Zcash address".into());
            }
            let total = m.withdraw(env, caller, &words(tokens)?, &amounts(a)).map_err(e)?;
            json!({ "payout": { "to": to, "zats": total as u64 } })
        }
        Command::Consolidate { tokens, amounts: a, bound, time_mode, payload } => {
            let t = m.consolidate(env, caller, &words(tokens)?, &amounts(a), *bound, *time_mode, *payload).map_err(e)?;
            json!({ "token": hex_word(t) })
        }
        Command::Attach { token, inst } => {
            m.attach(caller, codec::cid(parse_word(token)?), parse_word(inst)?).map_err(e)?;
            json!({})
        }
        Command::SetBaseSequence { id, seq } => {
            m.set_base_sequence(caller, *id, sequence(seq)?).map_err(e)?;
            json!({})
        }
        Command::SetSequence { id, seq } => {
            m.set_sequence(caller, *id, sequence(seq)?).map_err(e)?;
            json!({})
        }
        Command::SetGlobal { inst } => {
            m.set_global(caller, parse_word(inst)?).map_err(e)?;
            json!({})
        }
        Command::EscrowLock { token, recipient, arbiter, salt, expiry, mode } => {
            let c = m
                .escrow_lock(caller, parse_word(token)?, parse_addr(recipient)?, parse_addr(arbiter)?, parse_b32(salt)?, *expiry, *mode)
                .map_err(e)?;
            json!({ "commitment": hex_word(c) })
        }
        Command::EscrowRelease { commitment, token } => {
            let t = m.escrow_release(env, caller, parse_word(commitment)?, parse_word(token)?).map_err(e)?;
            json!({ "token": hex_word(t) })
        }
        Command::EscrowRefund { commitment, token } => {
            let t = m.escrow_refund(env, caller, parse_word(commitment)?, parse_word(token)?).map_err(e)?;
            json!({ "token": hex_word(t) })
        }
        Command::HtlcLock { token, commitment, expiry, mode } => {
            m.htlc_lock(caller, parse_word(token)?, parse_word(commitment)?, *expiry, *mode).map_err(e)?;
            json!({})
        }
        Command::HtlcClaim { sender, token, preimage } => {
            let t = m.htlc_claim(env, caller, parse_addr(sender)?, parse_word(token)?, parse_b32(preimage)?).map_err(e)?;
            json!({ "token": hex_word(t) })
        }
    })
}
