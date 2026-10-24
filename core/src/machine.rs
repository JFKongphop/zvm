//! The state machine: moneyvm's `ProgrammableMoney` + `Ledger.commit` flow, with the access rules a shared vault needs.
//!
//! Every public method is one command. A command either applies completely or not at all (the state is restored on
//! error), as an EVM revert would. Callers are already authenticated by the layer above (signed commands).

use crate::codec;
use crate::ext::{Escrow, EscrowLock, EscrowStage, Htlc, HtlcStage, Kind};
use crate::isa::{opcode, rd};
use crate::ledger::{Ledger, Token};
use crate::vm::{self, Frame, Sequence, DEFAULT_DISPATCH_SEQUENCE, DEFAULT_INTERCEPT_SEQUENCE};
use crate::{Addr, EXIT, Env, Error, U256};
use sha3::{Digest, Sha3_256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub network_id: u64,
    /// the vault's id, standing in for moneyvm's token contract address in commitments
    pub host: Addr,
    pub admin: Addr,
    pub ledger: Ledger,
    /// single instructions by cid; cid 0 is a global rule checked on every token
    pub instructions: BTreeMap<u128, U256>,
    /// base sequences by lineage id: the rules that follow money minted under that id
    pub bop_sequences: BTreeMap<u64, Sequence>,
    /// transaction sequences: intercept (before) and dispatch (after) a transfer
    pub txop_sequences: BTreeMap<u8, Sequence>,
    pub routes: [Option<Kind>; 16],
    /// who minted each lineage id; only they may set its base sequence
    pub issuers: BTreeMap<u64, Addr>,
    pub escrow: Escrow,
    pub htlc: Htlc,
}

/// Result of a transfer: the new tokens, one per input, in order.
pub type Moved = Vec<U256>;

impl Machine {
    pub fn new(network_id: u64, host: Addr, admin: Addr) -> Self {
        let mut routes = [None; 16];
        routes[1] = Some(Kind::Escrow);
        routes[2] = Some(Kind::Htlc);
        Machine {
            network_id,
            host,
            admin,
            ledger: Ledger::default(),
            instructions: BTreeMap::new(),
            bop_sequences: BTreeMap::new(),
            txop_sequences: BTreeMap::new(),
            routes,
            issuers: BTreeMap::new(),
            escrow: Escrow::default(),
            htlc: Htlc::default(),
        }
    }

    /// Runs `f` as one atomic command.
    fn atomic<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let snapshot = self.clone();
        let out = f(self);
        if out.is_err() {
            *self = snapshot;
        }
        out
    }

    fn env(&self, mut env: Env) -> Env {
        env.network_id = self.network_id;
        env
    }

    // ---- extension routing ---------------------------------------------------------------------------------------

    fn verify_extension(&self, inst: U256) -> Result<(), Error> {
        let dest = codec::rd(inst);
        if dest != rd::STD && self.routes[dest as usize].is_none() {
            return Err(Error::RtgBreak);
        }
        Ok(())
    }

    fn forward_single(escrow: &mut Escrow, htlc: &mut Htlc, kind: Option<Kind>, net: u64, host: &Addr, f: &Frame, inst: U256) -> Result<(bool, u32), Error> {
        match kind {
            Some(Kind::Escrow) => Ok((escrow.exec(net, host, f, inst), 0)),
            Some(Kind::Htlc) => Ok((htlc.exec(host, f, inst), 0)),
            None => Err(Error::RtgBreak),
        }
    }

    /// One single instruction (2CB-type) against one token.
    fn step_single(&mut self, inst: U256, f: &Frame) -> Result<u32, Error> {
        let dest = codec::rd(inst);
        if dest != rd::STD {
            let kind = self.routes[dest as usize];
            let (ok, payload) = Self::forward_single(&mut self.escrow, &mut self.htlc, kind, self.network_id, &self.host, f, inst)?;
            if !ok {
                return Err(Error::EBreak { rd: dest, subr: codec::subr(inst) });
            }
            Ok(payload)
        } else if vm::dalu(f, inst) {
            Ok(0)
        } else {
            Err(Error::AluBreak)
        }
    }

    /// A base sequence (CB-type steps) against one token.
    fn step_sequence(&mut self, seq: &Sequence, f: &Frame) -> Result<u32, Error> {
        let routes = self.routes;
        let (net, host) = (self.network_id, self.host);
        let (escrow, htlc) = (&mut self.escrow, &mut self.htlc);
        vm::run_sequence(seq, |inst| {
            let dest = codec::rd(inst);
            if dest != rd::STD {
                let (ok, data) = Self::forward_single(escrow, htlc, routes[dest as usize], net, &host, f, inst)?;
                Ok((ok, Some(data)))
            } else {
                Ok((vm::alu(f, inst), None))
            }
        })
    }

    /// Transaction-level sequences see sender, recipient, amount and the clock, not a single token.
    /// The built-in extensions have no batch mode (as in moneyvm), so routing a txop to them always fails.
    fn step_txop(&self, seq: &Sequence, f: &Frame) -> Result<(), Error> {
        vm::run_sequence(seq, |inst| {
            if codec::rd(inst) != rd::STD { Ok((false, None)) } else { Ok((vm::alu(f, inst), None)) }
        })
        .map(|_| ())
    }

    fn has_rules(&self, token: U256) -> bool {
        let id = codec::deserialize(token).id;
        self.bop_sequences.get(&id).is_some_and(|s| !s[0].is_zero())
            || self.instructions.get(&codec::cid(token)).is_some_and(|i| !i.is_zero())
    }

    // ---- moneyvm Ledger.commit -----------------------------------------------------------------------------------

    fn commit(
        &mut self,
        env: Env,
        from: Addr,
        to: Addr,
        tokens: &[U256],
        amounts: &[u128],
        intercept: u8,
        dispatch: u8,
    ) -> Result<Moved, Error> {
        if intercept == dispatch {
            return Err(Error::SameSequence);
        }
        if tokens.len() != amounts.len() || tokens.is_empty() {
            return Err(Error::InvalidArrayLength);
        }
        let total: u128 = amounts.iter().sum();
        let empty: Sequence = Default::default();

        let pre = *self.txop_sequences.get(&intercept).unwrap_or(&empty);
        self.step_txop(&pre, &Frame { token: U256::ZERO, sender: from, recipient: to, amount: total, env })?;

        let mut moved = Vec::with_capacity(tokens.len());
        for (&token, &amount) in tokens.iter().zip(amounts) {
            let f = Frame { token, sender: from, recipient: to, amount, env };

            let mut payload = 0u32;
            let global = *self.instructions.get(&0).unwrap_or(&U256::ZERO);
            if !global.is_zero() {
                payload = self.step_single(global, &f)?;
            }

            let base = *self.bop_sequences.get(&codec::deserialize(token).id).unwrap_or(&empty);
            let over = self.step_sequence(&base, &f)?;
            if codec::plw(payload) {
                payload = over;
            }

            let cid = codec::cid(token);
            if cid != 0 {
                let own = *self.instructions.get(&cid).unwrap_or(&U256::ZERO);
                let over = self.step_single(own, &f)?;
                if codec::plw(payload) {
                    payload = over;
                }
            }

            moved.push(self.ledger.reallocate(env.time, &from, &to, token, amount, payload)?);
        }

        let post = *self.txop_sequences.get(&dispatch).unwrap_or(&empty);
        self.step_txop(&post, &Frame { token: U256::ZERO, sender: from, recipient: to, amount: 0, env })?;
        Ok(moved)
    }

    // ---- commands --------------------------------------------------------------------------------------------------

    /// Shielded ZEC arrived at the vault: mint it as one token. Called by the sequencer for a confirmed deposit.
    /// `bound` and `time_mode` set an expiry or unlock time; `payload` is 31 bits of user data on the token.
    pub fn deposit(&mut self, env: Env, to: Addr, amount: u128, bound: u64, time_mode: u8, payload: u32) -> Result<U256, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            if amount == 0 {
                return Err(Error::InvalidParameters);
            }
            let token = m.ledger.issue(env.time, to, &[amount], &[bound], &[time_mode], &[payload])?[0];
            m.issuers.insert(codec::deserialize(token).id, to);
            Ok(token)
        })
    }

    pub fn transfer(&mut self, env: Env, caller: Addr, to: Addr, tokens: &[U256], amounts: &[u128]) -> Result<Moved, Error> {
        self.transfer_with(env, caller, to, tokens, amounts, DEFAULT_INTERCEPT_SEQUENCE, DEFAULT_DISPATCH_SEQUENCE)
    }

    pub fn transfer_with(
        &mut self,
        env: Env,
        caller: Addr,
        to: Addr,
        tokens: &[U256],
        amounts: &[u128],
        intercept: u8,
        dispatch: u8,
    ) -> Result<Moved, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            if to == EXIT {
                return Err(Error::InvalidParameters);
            }
            m.commit(env, caller, to, tokens, amounts, intercept, dispatch)
        })
    }

    /// Leaves the VM: moves the money to EXIT through every rule, then burns it. The sequencer pays the ZEC out
    /// only after this succeeds. Returns the total withdrawn.
    pub fn withdraw(&mut self, env: Env, caller: Addr, tokens: &[U256], amounts: &[u128]) -> Result<u128, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            let moved = m.commit(env, caller, EXIT, tokens, amounts, DEFAULT_INTERCEPT_SEQUENCE, DEFAULT_DISPATCH_SEQUENCE)?;
            m.ledger.retire(&EXIT, &moved, amounts)?;
            Ok(amounts.iter().sum())
        })
    }

    pub fn consolidate(&mut self, env: Env, caller: Addr, tokens: &[U256], amounts: &[u128], bound: u64, time_mode: u8, payload: u32) -> Result<U256, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            let rules: Vec<bool> = tokens.iter().map(|&t| m.has_rules(t)).collect();
            let lookup: BTreeMap<U256, bool> = tokens.iter().copied().zip(rules).collect();
            let merged = m.ledger.consolidate(env.time, &caller, tokens, amounts, |t| lookup[&t], bound, time_mode, payload)?;
            m.issuers.insert(codec::deserialize(merged).id, caller);
            Ok(merged)
        })
    }

    /// Attaches a single instruction to one of the caller's tokens. Rules are one-way: a token that already has an
    /// instruction cannot be given another, so an owner cannot strip an escrow or lock they agreed to.
    pub fn attach(&mut self, caller: Addr, cid: u128, inst: U256) -> Result<(), Error> {
        self.atomic(|m| {
            let token = *m.ledger.by_cid.get(&cid).ok_or(Error::UnknownToken)?;
            let t: &Token = &m.ledger.tokens[&token];
            if t.owner != caller || t.balance == 0 {
                return Err(Error::Unauthorized);
            }
            if m.instructions.get(&cid).is_some_and(|i| !i.is_zero()) {
                return Err(Error::AlreadySet);
            }
            if codec::opcode(inst) != opcode::TOP {
                return Err(Error::EncBreak);
            }
            vm::verify_encoding(inst)?;
            m.verify_extension(inst)?;
            m.instructions.insert(cid, inst);
            Ok(())
        })
    }

    /// Sets the rules that follow every token of a lineage. Only its issuer, and only once.
    pub fn set_base_sequence(&mut self, caller: Addr, id: u64, seq: Sequence) -> Result<(), Error> {
        self.atomic(|m| {
            if m.issuers.get(&id) != Some(&caller) {
                return Err(Error::Unauthorized);
            }
            if m.bop_sequences.contains_key(&id) {
                return Err(Error::AlreadySet);
            }
            m.write_sequence(&seq, opcode::BOP)?;
            m.bop_sequences.insert(id, seq);
            Ok(())
        })
    }

    /// Vault-wide transfer rules (e.g. a compliance check). Admin only.
    pub fn set_sequence(&mut self, caller: Addr, id: u8, seq: Sequence) -> Result<(), Error> {
        self.atomic(|m| {
            if caller != m.admin {
                return Err(Error::Unauthorized);
            }
            m.write_sequence(&seq, opcode::TXOP)?;
            m.txop_sequences.insert(id, seq);
            Ok(())
        })
    }

    /// A rule every token must pass, at cid 0. Admin only.
    pub fn set_global(&mut self, caller: Addr, inst: U256) -> Result<(), Error> {
        self.atomic(|m| {
            if caller != m.admin {
                return Err(Error::Unauthorized);
            }
            if !inst.is_zero() {
                if codec::opcode(inst) != opcode::TOP {
                    return Err(Error::EncBreak);
                }
                vm::verify_encoding(inst)?;
                m.verify_extension(inst)?;
            }
            m.instructions.insert(0, inst);
            Ok(())
        })
    }

    fn write_sequence(&self, seq: &Sequence, expected_op: u8) -> Result<(), Error> {
        for &inst in seq {
            if !inst.is_zero() {
                if codec::opcode(inst) != expected_op {
                    return Err(Error::EncBreak);
                }
                vm::verify_encoding(inst)?;
                self.verify_extension(inst)?;
            }
        }
        Ok(())
    }

    // ---- escrow ----------------------------------------------------------------------------------------------------

    /// Locks `amount` of the caller's `token` for `recipient`, released or refunded by `arbiter`.
    /// The whole token is the escrowed amount, so split it first if needed. Returns the commitment.
    pub fn escrow_lock(
        &mut self,
        caller: Addr,
        token: U256,
        recipient: Addr,
        arbiter: Addr,
        salt: [u8; 32],
        expiry: u64,
        mode: u8,
    ) -> Result<U256, Error> {
        self.atomic(|m| {
            let t = m.ledger.tokens.get(&token).ok_or(Error::UnknownToken)?;
            if t.owner != caller || t.balance == 0 || recipient == EXIT {
                return Err(Error::InvalidParameters);
            }
            let commitment = Escrow::commitment(m.network_id, &m.host, &caller, &recipient, &arbiter, token, t.balance, &salt);
            if m.escrow.locks.contains_key(&commitment) {
                return Err(Error::CommitmentAlreadyConsumed);
            }
            m.escrow.locks.insert(commitment, EscrowLock { sender: caller, recipient, arbiter, salt, stage: EscrowStage::Locked });
            let inst = codec::encode_dcb(&codec::Dcb { rd: 1, subr: mode, imm_1: commitment, imm_2: expiry, ..Default::default() })?;
            m.attach_unchecked(token, inst)?;
            Ok(commitment)
        })
    }

    fn attach_unchecked(&mut self, token: U256, inst: U256) -> Result<(), Error> {
        let cid = codec::cid(token);
        if self.instructions.get(&cid).is_some_and(|i| !i.is_zero()) {
            return Err(Error::AlreadySet);
        }
        self.instructions.insert(cid, inst);
        Ok(())
    }

    fn escrow_settle(&mut self, env: Env, caller: Addr, commitment: U256, token: U256, release: bool) -> Result<U256, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            let e = m.escrow.locks.get_mut(&commitment).ok_or(Error::UnknownToken)?;
            if caller != e.arbiter {
                return Err(Error::UnauthorizedArbiter);
            }
            if e.stage != EscrowStage::Locked {
                return Err(Error::CommitmentAlreadyConsumed);
            }
            e.stage = if release { EscrowStage::StagedRelease } else { EscrowStage::StagedRefund };
            let (sender, to) = (e.sender, if release { e.recipient } else { e.sender });
            let amount = m.ledger.tokens.get(&token).ok_or(Error::UnknownToken)?.balance;
            let moved = m.commit(env, sender, to, &[token], &[amount], DEFAULT_INTERCEPT_SEQUENCE, DEFAULT_DISPATCH_SEQUENCE)?;
            if m.escrow.locks[&commitment].stage != EscrowStage::Consumed {
                return Err(Error::CommitmentNotConsumed);
            }
            Ok(moved[0])
        })
    }

    pub fn escrow_release(&mut self, env: Env, caller: Addr, commitment: U256, token: U256) -> Result<U256, Error> {
        self.escrow_settle(env, caller, commitment, token, true)
    }

    pub fn escrow_refund(&mut self, env: Env, caller: Addr, commitment: U256, token: U256) -> Result<U256, Error> {
        self.escrow_settle(env, caller, commitment, token, false)
    }

    // ---- HTLC ------------------------------------------------------------------------------------------------------

    /// Locks the caller's whole `token` behind a commitment the sender computed off-chain from a secret preimage
    /// (`Htlc::commitment`). The recipient claims it with the preimage before `expiry`.
    pub fn htlc_lock(&mut self, caller: Addr, token: U256, commitment: U256, expiry: u64, mode: u8) -> Result<(), Error> {
        self.atomic(|m| {
            let t = m.ledger.tokens.get(&token).ok_or(Error::UnknownToken)?;
            if t.owner != caller || t.balance == 0 {
                return Err(Error::InvalidParameters);
            }
            let inst = codec::encode_dcb(&codec::Dcb { rd: 2, subr: mode, imm_1: commitment, imm_2: expiry, ..Default::default() })?;
            m.attach_unchecked(token, inst)
        })
    }

    /// The recipient (caller) claims `token` from `sender` by revealing the preimage.
    pub fn htlc_claim(&mut self, env: Env, caller: Addr, sender: Addr, token: U256, preimage: [u8; 32]) -> Result<U256, Error> {
        let env = self.env(env);
        self.atomic(|m| {
            let amount = m.ledger.tokens.get(&token).ok_or(Error::UnknownToken)?.balance;
            let commitment = Htlc::commitment(m.network_id, &m.host, &sender, &caller, token, amount, &preimage);
            let key = Htlc::context_key(&m.host, &sender, token, commitment);
            if m.htlc.stage(&key) != HtlcStage::Unused {
                return Err(Error::CommitmentAlreadyConsumed);
            }
            m.htlc.stages.insert(key, HtlcStage::Staged);
            let moved = m.commit(env, sender, caller, &[token], &[amount], DEFAULT_INTERCEPT_SEQUENCE, DEFAULT_DISPATCH_SEQUENCE)?;
            if m.htlc.stage(&key) != HtlcStage::Consumed {
                return Err(Error::CommitmentNotConsumed);
            }
            Ok(moved[0])
        })
    }

    // ---- reads -----------------------------------------------------------------------------------------------------

    /// Tokens an owner holds with a non-zero balance.
    pub fn holdings(&self, owner: &Addr) -> Vec<(U256, u128)> {
        self.ledger.tokens.iter().filter(|(_, t)| &t.owner == owner && t.balance > 0).map(|(k, t)| (*k, t.balance)).collect()
    }

    /// A hash of the whole state. Two machines that replayed the same commands have the same root.
    pub fn state_root(&self) -> [u8; 32] {
        let mut h = Sha3_256::new();
        h.update(b"zvm-state-v1");
        h.update(self.network_id.to_be_bytes());
        h.update(self.host);
        h.update(self.admin);
        h.update(self.ledger.total_supply.to_be_bytes());
        h.update(self.ledger.id.to_be_bytes());
        for (k, t) in &self.ledger.tokens {
            h.update(k.to_be_bytes::<32>());
            h.update(t.balance.to_be_bytes());
            h.update(t.owner);
            h.update(t.slice.to_be_bytes());
        }
        for (cid, inst) in &self.instructions {
            h.update(cid.to_be_bytes());
            h.update(inst.to_be_bytes::<32>());
        }
        for (id, seq) in &self.bop_sequences {
            h.update(id.to_be_bytes());
            seq.iter().for_each(|i| h.update(i.to_be_bytes::<32>()));
        }
        for (id, seq) in &self.txop_sequences {
            h.update([*id]);
            seq.iter().for_each(|i| h.update(i.to_be_bytes::<32>()));
        }
        for (id, a) in &self.issuers {
            h.update(id.to_be_bytes());
            h.update(a);
        }
        for (c, e) in &self.escrow.locks {
            h.update(c.to_be_bytes::<32>());
            h.update(e.sender);
            h.update(e.recipient);
            h.update(e.arbiter);
            h.update(e.salt);
            h.update([e.stage as u8]);
        }
        for (k, s) in &self.htlc.stages {
            h.update(k);
            h.update([*s as u8]);
        }
        h.finalize().into()
    }
}
