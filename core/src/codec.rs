//! Instruction and token encodings, ported from moneyvm `src/arch/Codec.sol`.
//! Copyright 2026 Sirawit Techavanitch (moneyvm), Apache-2.0. Rust port for zvm.
//!
//! All layouts are little-endian bit fields inside one 256-bit word (LSB first).

use crate::Error;
use crate::isa::tmd;
use ruint::aliases::U256;

pub fn mask(bits: usize) -> U256 {
    if bits >= 256 { U256::MAX } else { (U256::from(1u8) << bits) - U256::from(1u8) }
}

/// Bits `[at, at + bits)` of `word`.
pub fn field(word: U256, at: usize, bits: usize) -> U256 {
    (word >> at) & mask(bits)
}

pub fn low_u64(x: U256) -> u64 {
    x.as_limbs()[0]
}

fn small(word: U256, at: usize, bits: usize) -> u8 {
    low_u64(field(word, at, bits)) as u8
}

pub fn opcode(inst: U256) -> u8 {
    small(inst, 0, 2)
}
pub fn rd(inst: U256) -> u8 {
    small(inst, 2, 4)
}
pub fn subr(inst: U256) -> u8 {
    small(inst, 6, 4)
}

// ---- 2CB-type (opcode 0, "top"): a single instruction attached to one token ---------------------------------------
// [0:1] opcode · [2:5] rd · [6:9] subr · [10:169] imm_1 (160) · [170:173] funct4_1 · [174:177] rs1 · [178:181] rs2
// [182:185] funct4_2 · [186:249] imm_2 (64) · [250] gate (0 AND, 1 OR) · [251:255] reserved

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dcb {
    pub rd: u8,
    pub subr: u8,
    pub imm_1: U256,
    pub funct4_1: u8,
    pub rs1: u8,
    pub rs2: u8,
    pub funct4_2: u8,
    pub imm_2: u64,
    pub gate: u8,
}

pub fn encode_dcb(d: &Dcb) -> Result<U256, Error> {
    let mut p = U256::ZERO;
    p |= U256::from(d.rd & 0xF) << 2;
    p |= U256::from(d.subr & 0xF) << 6;
    p |= (d.imm_1 & mask(160)) << 10;
    p |= U256::from(d.funct4_1 & 0xF) << 170;
    p |= U256::from(d.rs1 & 0xF) << 174;
    p |= U256::from(d.rs2 & 0xF) << 178;
    p |= U256::from(d.funct4_2 & 0xF) << 182;
    p |= U256::from(d.imm_2) << 186;
    p |= U256::from(d.gate & 1) << 250;
    crate::vm::verify_encoding(p)?;
    Ok(p)
}

pub fn decode_dcb(inst: U256) -> Result<Dcb, Error> {
    if opcode(inst) != 0 {
        return Err(Error::InvalidInstructionFormat);
    }
    Ok(Dcb {
        rd: rd(inst),
        subr: subr(inst),
        imm_1: field(inst, 10, 160),
        funct4_1: small(inst, 170, 4),
        rs1: small(inst, 174, 4),
        rs2: small(inst, 178, 4),
        funct4_2: small(inst, 182, 4),
        imm_2: low_u64(field(inst, 186, 64)),
        gate: small(inst, 250, 1),
    })
}

pub fn imm_1(inst: U256) -> U256 {
    field(inst, 10, 160)
}
pub fn imm_2(inst: U256) -> u64 {
    low_u64(field(inst, 186, 64))
}

// ---- CB-type (opcode 1 "bop" / 2 "txop"): one step of a sequence ---------------------------------------------------
// [0:1] opcode · [2:5] rd · [6:9] subr · [10:13] funct4 · [14:17] rs1 · [18:249] imm (232) · [250:252] ctl · [253:255] rsvd

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cb {
    pub opcode: u8,
    pub rd: u8,
    pub subr: u8,
    pub funct4: u8,
    pub rs1: u8,
    pub imm: U256,
    pub ctl: u8,
}

pub fn encode_cb(c: &Cb) -> Result<U256, Error> {
    if c.opcode == 0 || c.opcode > 2 {
        return Err(Error::InvalidInstructionFormat);
    }
    let mut p = U256::from(c.opcode & 3);
    p |= U256::from(c.rd & 0xF) << 2;
    p |= U256::from(c.subr & 0xF) << 6;
    p |= U256::from(c.funct4 & 0xF) << 10;
    p |= U256::from(c.rs1 & 0xF) << 14;
    p |= (c.imm & mask(232)) << 18;
    p |= U256::from(c.ctl & 7) << 250;
    crate::vm::verify_encoding(p)?;
    Ok(p)
}

pub fn decode_cb(inst: U256) -> Result<Cb, Error> {
    let op = opcode(inst);
    if op == 0 || op > 2 {
        return Err(Error::InvalidInstructionFormat);
    }
    Ok(Cb {
        opcode: op,
        rd: rd(inst),
        subr: subr(inst),
        funct4: small(inst, 10, 4),
        rs1: small(inst, 14, 4),
        imm: field(inst, 18, 232),
        ctl: small(inst, 250, 3),
    })
}

/// rs1 sits in a different place for each format.
pub fn rs1(inst: U256) -> u8 {
    if opcode(inst) == 0 { small(inst, 174, 4) } else { small(inst, 14, 4) }
}

// ---- UNI-type: opcode · rd · subr · 246-bit immediate, for extensions that want the whole word ---------------------

pub fn encode_uni(op: u8, rd: u8, subr: u8, immx: U256) -> Result<U256, Error> {
    if op > 2 {
        return Err(Error::InvalidInstructionFormat);
    }
    Ok(U256::from(op & 3) | (U256::from(rd & 0xF) << 2) | (U256::from(subr & 0xF) << 6) | ((immx & mask(246)) << 10))
}

pub fn immx(inst: U256) -> U256 {
    field(inst, 10, 246)
}

// ---- Token id: every unit of money is named by its own lineage and rules -----------------------------------------
// [0:63] id · [64:95] seq · [96:127] slc · [128:175] iat · [176:223] ts · [224] tmd · [225:255] pl
// The low 128 bits (id, seq, slc) are the `cid` that single instructions are attached to.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenCtx {
    pub id: u64,
    pub seq: u32,
    pub slc: u32,
    pub tmd: u8,
    pub iat: u64,
    pub ts: u64,
    pub pl: u32,
}

impl TokenCtx {
    pub fn cid(&self) -> u128 {
        cid_of(self.id, self.seq, self.slc)
    }
}

pub fn cid_of(id: u64, seq: u32, slc: u32) -> u128 {
    id as u128 | (seq as u128) << 64 | (slc as u128) << 96
}

pub fn serialize(id: u64, seq: u32, slc: u32, time_mode: u8, iat: u64, ts: u64, pl: u32) -> Result<U256, Error> {
    if time_mode > 1 {
        return Err(Error::InvalidTokenExpiration);
    }
    if time_mode == tmd::TLK && ts != 0 && iat >= ts {
        return Err(Error::InvalidTokenExpiration);
    }
    let w48 = (1u64 << 48) - 1;
    Ok(U256::from(id)
        | U256::from(seq) << 64
        | U256::from(slc) << 96
        | U256::from(iat & w48) << 128
        | U256::from(ts & w48) << 176
        | U256::from(time_mode & 1) << 224
        | U256::from(pl & 0x7FFF_FFFF) << 225)
}

pub fn deserialize(token: U256) -> TokenCtx {
    TokenCtx {
        id: low_u64(field(token, 0, 64)),
        seq: low_u64(field(token, 64, 32)) as u32,
        slc: low_u64(field(token, 96, 32)) as u32,
        iat: low_u64(field(token, 128, 48)),
        ts: low_u64(field(token, 176, 48)),
        tmd: low_u64(field(token, 224, 1)) as u8,
        pl: low_u64(field(token, 225, 31)) as u32,
    }
}

pub fn cid(token: U256) -> u128 {
    let l = token.as_limbs();
    l[0] as u128 | (l[1] as u128) << 64
}

/// A payload is writable while bit 30 is clear; an extension sets bit 30 to lock what it wrote.
pub fn plw(payload: u32) -> bool {
    payload & 0x4000_0000 == 0
}
