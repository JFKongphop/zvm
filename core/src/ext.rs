//! Built-in extensions, ported from moneyvm `XEscrowUnit.sol` and `XHTLCUnit.sol`.
//! Copyright 2026 Sirawit Techavanitch (moneyvm), Apache-2.0. Rust port for zvm.
//!
//! An extension sits in a slot (1..=15). A token whose attached instruction routes to that slot can only move when
//! the extension agrees. Commitments hash the same fields as moneyvm (keccak256, packed), with the network id in
//! place of the EVM chain id and the vault id in place of the token contract.

use crate::codec;
use crate::vm::Frame;
use crate::{Addr, U256};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Escrow,
    Htlc,
}

pub mod escrow_mode {
    /// arbiter approves or refunds; after expiry the sender can take it back
    pub const STD: u8 = 0;
    /// arbiter can only approve; after expiry the sender can take it back
    pub const RELEASE_ONLY: u8 = 1;
    /// arbiter approves or refunds; no expiry
    pub const PERPETUAL: u8 = 2;
    pub const TRAP: u8 = 3;
}

pub mod htlc_mode {
    /// claim with the preimage before expiry; sender takes it back after
    pub const STD: u8 = 0;
    pub const HASHLOCK_ONLY: u8 = 1;
    pub const TIMELOCK_ONLY: u8 = 2;
    pub const TRAP: u8 = 3;
}

fn word(x: U256) -> [u8; 32] {
    x.to_be_bytes()
}

/// The low 160 bits of a keccak256 hash, as moneyvm's `uint160(uint256(keccak256(...)))`.
fn h160(parts: &[&[u8]]) -> U256 {
    let mut k = Keccak256::new();
    for p in parts {
        k.update(p);
    }
    U256::from_be_slice(&k.finalize()[12..])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EscrowStage {
    Unused,
    Locked,
    StagedRelease,
    StagedRefund,
    Consumed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EscrowLock {
    pub sender: Addr,
    pub recipient: Addr,
    pub arbiter: Addr,
    pub salt: [u8; 32],
    pub stage: EscrowStage,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Escrow {
    pub locks: BTreeMap<U256, EscrowLock>,
}

impl Escrow {
    pub fn commitment(
        network_id: u64,
        host: &Addr,
        sender: &Addr,
        recipient: &Addr,
        arbiter: &Addr,
        token: U256,
        amount: u128,
        salt: &[u8; 32],
    ) -> U256 {
        h160(&[
            &word(U256::from(network_id)),
            host,
            sender,
            recipient,
            arbiter,
            &word(token),
            &word(U256::from(amount)),
            salt,
        ])
    }

    /// Runs when a token carrying an escrow instruction moves. `imm_1` is the commitment, `imm_2` the expiry.
    pub fn exec(&mut self, network_id: u64, host: &Addr, f: &Frame, inst: U256) -> bool {
        let mode = codec::subr(inst);
        if mode >= escrow_mode::TRAP {
            return false;
        }
        let expected = codec::imm_1(inst);
        let expiry = codec::imm_2(inst);
        let Some(e) = self.locks.get_mut(&expected) else { return false };
        if f.sender != e.sender {
            return false;
        }
        if Self::commitment(network_id, host, &f.sender, &e.recipient, &e.arbiter, f.token, f.amount, &e.salt) != expected {
            return false;
        }
        if mode != escrow_mode::PERPETUAL && f.env.time >= expiry {
            // after expiry, only the sender taking it back
            if f.recipient != f.sender || e.stage != EscrowStage::Locked {
                return false;
            }
            e.stage = EscrowStage::Consumed;
            return true;
        }
        if e.stage == EscrowStage::StagedRelease && f.recipient == e.recipient {
            e.stage = EscrowStage::Consumed;
            return true;
        }
        if mode != escrow_mode::RELEASE_ONLY && e.stage == EscrowStage::StagedRefund && f.recipient == f.sender {
            e.stage = EscrowStage::Consumed;
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HtlcStage {
    Unused,
    Staged,
    Consumed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Htlc {
    pub stages: BTreeMap<[u8; 32], HtlcStage>,
}

impl Htlc {
    pub fn commitment(
        network_id: u64,
        host: &Addr,
        sender: &Addr,
        recipient: &Addr,
        token: U256,
        amount: u128,
        preimage: &[u8; 32],
    ) -> U256 {
        h160(&[&word(U256::from(network_id)), host, sender, recipient, &word(token), &word(U256::from(amount)), preimage])
    }

    /// keccak256(abi.encode(host, sender, token, commitment)): each field padded to 32 bytes.
    pub fn context_key(host: &Addr, sender: &Addr, token: U256, commitment: U256) -> [u8; 32] {
        let pad = |a: &Addr| {
            let mut w = [0u8; 32];
            w[12..].copy_from_slice(a);
            w
        };
        let mut k = Keccak256::new();
        k.update(pad(host));
        k.update(pad(sender));
        k.update(word(token));
        k.update(word(commitment));
        k.finalize().into()
    }

    pub fn stage(&self, key: &[u8; 32]) -> HtlcStage {
        *self.stages.get(key).unwrap_or(&HtlcStage::Unused)
    }

    /// Runs when a token carrying an HTLC instruction moves. `imm_1` is the commitment, `imm_2` the expiry.
    pub fn exec(&mut self, host: &Addr, f: &Frame, inst: U256) -> bool {
        let mode = codec::subr(inst);
        if mode >= htlc_mode::TRAP {
            return false;
        }
        let expected = codec::imm_1(inst);
        let expiry = codec::imm_2(inst);
        let key = Self::context_key(host, &f.sender, f.token, expected);
        let stage = self.stage(&key);

        if mode != htlc_mode::HASHLOCK_ONLY && f.env.time >= expiry {
            if f.recipient != f.sender || stage != HtlcStage::Unused {
                return false;
            }
            self.stages.insert(key, HtlcStage::Consumed);
            return true;
        }
        if mode != htlc_mode::TIMELOCK_ONLY && stage == HtlcStage::Staged {
            self.stages.insert(key, HtlcStage::Consumed);
            return true;
        }
        false
    }
}
