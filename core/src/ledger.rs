//! Token balances, ported from moneyvm `src/state/Ledger.sol`.
//! Copyright 2026 Sirawit Techavanitch (moneyvm), Apache-2.0. Rust port for zvm.
//!
//! Money is held as distinct tokens (like UTXOs). Moving part of a token splits it: the moved part becomes a new token
//! whose id keeps the lineage (`id`) and records the split (`seq`, `slc`).

use crate::codec::{self, TokenCtx};
use crate::isa::tmd;
use crate::{Addr, Error, U256, token_error};
use std::collections::BTreeMap;

const SEQ_START_AT: u32 = 1;
const SLC_START_AT: u32 = 1;
const MAX_SLC: u32 = u32::MAX;
const MAX_SEQ: u32 = u16::MAX as u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub balance: u128,
    pub owner: Addr,
    pub slice: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub tokens: BTreeMap<U256, Token>,
    /// cid (low 128 bits) → token, for attaching instructions by cid
    pub by_cid: BTreeMap<u128, U256>,
    pub total_supply: u128,
    pub id: u64,
}

impl Ledger {
    fn insert(&mut self, token: U256, t: Token) {
        self.by_cid.insert(codec::cid(token), token);
        self.tokens.insert(token, t);
    }

    pub fn balance_of(&self, owner: &Addr, token: U256) -> u128 {
        match self.tokens.get(&token) {
            Some(t) if &t.owner == owner => t.balance,
            _ => 0,
        }
    }

    /// Mints new tokens. `bounds[i]` is an expiry (time mode 0) or unlock time (time mode 1); 0 means none.
    pub fn issue(
        &mut self,
        now: u64,
        to: Addr,
        amounts: &[u128],
        bounds: &[u64],
        flags: &[u8],
        payloads: &[u32],
    ) -> Result<Vec<U256>, Error> {
        let n = amounts.len();
        if bounds.len() != n || flags.len() != n || payloads.len() != n {
            return Err(Error::InvalidArrayLength);
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if bounds[i] != 0 && bounds[i] < now {
                return Err(Error::InvalidToken(token_error::EXPIRED));
            }
            self.id += 1;
            let token = codec::serialize(self.id, SEQ_START_AT, SLC_START_AT, flags[i], now, bounds[i], payloads[i])?;
            self.insert(token, Token { balance: amounts[i], owner: to, slice: SLC_START_AT });
            self.total_supply += amounts[i];
            out.push(token);
        }
        Ok(out)
    }

    pub fn retire(&mut self, owner: &Addr, tokens: &[U256], amounts: &[u128]) -> Result<(), Error> {
        if tokens.len() != amounts.len() {
            return Err(Error::InvalidArrayLength);
        }
        for (token, &amount) in tokens.iter().zip(amounts) {
            let t = self.tokens.get_mut(token).ok_or(Error::UnknownToken)?;
            if &t.owner != owner {
                return Err(Error::InvalidToken(token_error::UNAUTHORIZED));
            }
            if t.balance < amount {
                return Err(Error::InsufficientBalance);
            }
            t.balance -= amount;
            self.total_supply -= amount;
        }
        Ok(())
    }

    /// Moves `amount` of `token` from `from` to `to`, minting the moved part as a new token.
    pub fn reallocate(&mut self, now: u64, from: &Addr, to: &Addr, token: U256, amount: u128, payload: u32) -> Result<U256, Error> {
        let t = self.tokens.get(&token).ok_or(Error::UnknownToken)?.clone();
        if &t.owner != from {
            return Err(Error::InvalidToken(token_error::UNAUTHORIZED));
        }
        if t.balance < amount {
            return Err(Error::InsufficientBalance);
        }
        let ctx: TokenCtx = codec::deserialize(token);
        if ctx.tmd == tmd::TLK && now < ctx.ts {
            return Err(Error::InvalidToken(token_error::LOCKED));
        }
        // moneyvm compares `timestamp > timestamp` here, so expired money still moved; the port checks the clock.
        if ctx.tmd == tmd::EXP && ctx.ts != 0 && now > ctx.ts {
            return Err(Error::InvalidToken(token_error::EXPIRED));
        }

        let mut new_id = ctx.id;
        let mut new_seq = ctx.seq;
        let mut new_slc;
        let mut parent_slice = t.slice;
        if t.balance == amount {
            new_seq += 1;
            new_slc = SLC_START_AT;
        } else {
            new_slc = parent_slice + 1;
            if new_slc == MAX_SLC {
                new_slc = SLC_START_AT;
                new_seq += 1;
                if new_seq == MAX_SEQ {
                    new_seq = SEQ_START_AT;
                    self.id += 1;
                    new_id = self.id;
                }
                new_slc += 1;
            } else {
                parent_slice = new_slc;
            }
        }

        // Several commands share one Zcash block time, so two splits can serialize to the same id. moneyvm would
        // overwrite the first; the port moves to the next free slice instead.
        let mut reallocated = codec::serialize(new_id, new_seq, new_slc, 0, now, 0, payload)?;
        while self.tokens.contains_key(&reallocated) {
            new_slc = new_slc.checked_add(1).ok_or(Error::InvalidToken(token_error::UNAUTHORIZED))?;
            if t.balance != amount {
                parent_slice = new_slc;
            }
            reallocated = codec::serialize(new_id, new_seq, new_slc, 0, now, 0, payload)?;
        }

        let parent = self.tokens.get_mut(&token).expect("checked above");
        parent.balance -= amount;
        parent.slice = parent_slice;
        self.insert(reallocated, Token { balance: amount, owner: *to, slice: SLC_START_AT });
        Ok(reallocated)
    }

    /// Merges plain tokens (no time bound, no rules) into one new token.
    pub fn consolidate(
        &mut self,
        now: u64,
        owner: &Addr,
        tokens: &[U256],
        amounts: &[u128],
        has_rules: impl Fn(U256) -> bool,
        bound: u64,
        flag: u8,
        payload: u32,
    ) -> Result<U256, Error> {
        if tokens.len() != amounts.len() {
            return Err(Error::InvalidArrayLength);
        }
        for &token in tokens {
            if codec::deserialize(token).ts != 0 {
                return Err(Error::InvalidToken(token_error::WITH_TIMESTAMP));
            }
            let t = self.tokens.get(&token).ok_or(Error::UnknownToken)?;
            if &t.owner != owner {
                return Err(Error::InvalidToken(token_error::UNAUTHORIZED));
            }
            if has_rules(token) {
                return Err(Error::InvalidToken(token_error::WITH_INSTRUCTION));
            }
        }
        let mut total = 0u128;
        for (token, &amount) in tokens.iter().zip(amounts) {
            let t = self.tokens.get_mut(token).expect("checked above");
            if t.balance < amount {
                return Err(Error::InsufficientBalance);
            }
            t.balance -= amount;
            total += amount;
        }
        self.id += 1;
        let merged = codec::serialize(self.id, SEQ_START_AT, SLC_START_AT, flag, now, bound, payload)?;
        self.insert(merged, Token { balance: total, owner: *owner, slice: SLC_START_AT });
        Ok(merged)
    }
}
