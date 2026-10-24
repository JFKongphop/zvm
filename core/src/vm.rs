//! The coprocessor: ALU, control flow and encoding checks, ported from moneyvm `src/vm/CoprocessorVM.sol`.
//! Copyright 2026 Sirawit Techavanitch (moneyvm), Apache-2.0. Rust port for zvm.
//!
//! Extension calls are passed in as a closure, so this file stays pure; `machine.rs` wires them to real extensions.

use crate::codec::{self, field, low_u64};
use crate::isa::{ctl, funct4, opcode, rd, rs1};
use crate::{Addr, Env, Error, addr_word};
use ruint::aliases::U256;

pub const MAX_STEPS: usize = 12;
pub const MAX_SEQUENCE_SIZE: usize = 6;
pub const DEFAULT_INTERCEPT_SEQUENCE: u8 = 0;
pub const DEFAULT_DISPATCH_SEQUENCE: u8 = 128;

pub type Sequence = [U256; MAX_SEQUENCE_SIZE];

/// What an instruction can see about the transfer it is judging.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub token: U256,
    pub sender: Addr,
    pub recipient: Addr,
    pub amount: u128,
    pub env: Env,
}

/// Reads a source register. Follows the register numbers in `ISA.sol` (tmd = 4, iat = 5, ts = 6).
pub fn read_rs(f: &Frame, rs: u8) -> U256 {
    let t = f.token;
    match rs {
        rs1::ID => field(t, 0, 64),
        rs1::SEQ => field(t, 64, 32),
        rs1::SLC => field(t, 96, 32),
        rs1::TMD => field(t, 224, 1),
        rs1::IAT => field(t, 128, 48),
        rs1::TS => field(t, 176, 48),
        rs1::PL => field(t, 225, 31),
        rs1::SNDR => addr_word(&f.sender),
        rs1::RCPT => addr_word(&f.recipient),
        rs1::AMT => U256::from(f.amount),
        rs1::CLK => U256::from(f.env.time),
        rs1::EPOCH => U256::from(f.env.height),
        rs1::NETWORK_ID => U256::from(f.env.network_id),
        _ => U256::ZERO,
    }
}

pub fn evaluate(lhs: U256, rhs: U256, f4: u8) -> bool {
    let bounds = || (field(rhs, 0, 72), field(rhs, 72, 72));
    match f4 {
        funct4::NOP => true,
        funct4::BEQ => lhs == rhs,
        funct4::NEQ => lhs != rhs,
        funct4::BLTU => lhs < rhs,
        funct4::BLEU => lhs <= rhs,
        funct4::BGTU => lhs > rhs,
        funct4::BGEU => lhs >= rhs,
        funct4::BEQZ => lhs.is_zero(),
        funct4::BNEZ => !lhs.is_zero(),
        funct4::BINB => {
            let (min, max) = bounds();
            lhs >= min && lhs <= max
        }
        funct4::BOOB => {
            let (min, max) = bounds();
            lhs < min || lhs > max
        }
        _ => false,
    }
}

/// CB-type ALU step: one condition on one register.
pub fn alu(f: &Frame, inst: U256) -> bool {
    let r1 = low_u64(field(inst, 14, 4)) as u8;
    let f4 = low_u64(field(inst, 10, 4)) as u8;
    evaluate(read_rs(f, r1), field(inst, 18, 232), f4)
}

/// 2CB-type ALU: up to two conditions joined by AND / OR, with short-circuit.
pub fn dalu(f: &Frame, inst: U256) -> bool {
    let r1 = low_u64(field(inst, 174, 4)) as u8;
    let f4_1 = low_u64(field(inst, 170, 4)) as u8;
    let res1 = evaluate(read_rs(f, r1), codec::imm_1(inst), f4_1);

    let f4_2 = low_u64(field(inst, 182, 4)) as u8;
    if f4_2 == 0 {
        return res1;
    }
    let gate = low_u64(field(inst, 250, 1));
    if gate == 0 && !res1 {
        return false;
    }
    if gate == 1 && res1 {
        return true;
    }
    let r2 = low_u64(field(inst, 178, 4)) as u8;
    evaluate(read_rs(f, r2), U256::from(codec::imm_2(inst)), f4_2)
}

pub struct Registers {
    pub pc: usize,
    pub prev_pc: usize,
    pub steps: usize,
    pub ac: bool,
}

impl Registers {
    pub fn new() -> Self {
        Registers { pc: 0, prev_pc: usize::MAX, steps: 0, ac: true }
    }
}

/// Applies the step's control code. Returns false when the sequence should stop.
pub fn control_flow(r: &mut Registers, code: u8, imm: U256, result: bool) -> Result<bool, Error> {
    let jump = |r: &mut Registers| -> Result<(), Error> {
        let target = low_u64(imm & U256::from(0xFFu8)) as usize;
        if target == r.pc || target == r.prev_pc {
            return Err(Error::JmpBreak);
        }
        r.prev_pc = r.pc;
        r.pc = target;
        Ok(())
    };
    let next = |r: &mut Registers| {
        r.prev_pc = r.pc;
        r.pc += 1;
    };
    match code {
        ctl::END => {
            r.ac = r.ac && result;
            return Ok(false);
        }
        ctl::AND => {
            r.ac = r.ac && result;
            next(r);
        }
        ctl::OR => {
            r.ac = r.ac || result;
            next(r);
        }
        ctl::XOR => {
            r.ac ^= result;
            next(r);
        }
        ctl::REQ => {
            if !result {
                return Err(Error::CtlBreak(4));
            }
            next(r);
        }
        ctl::JMP => jump(r)?,
        ctl::JMPT => {
            if r.ac { jump(r)? } else { next(r) }
        }
        ctl::JMPF => {
            if !r.ac { jump(r)? } else { next(r) }
        }
        other => return Err(Error::CtlBreak(other)),
    }
    Ok(true)
}

/// Runs a sequence. `step` executes one instruction and returns its result plus an optional payload.
pub fn run_sequence(
    seq: &Sequence,
    mut step: impl FnMut(U256) -> Result<(bool, Option<u32>), Error>,
) -> Result<u32, Error> {
    let mut r = Registers::new();
    let mut payload = 0u32;
    while r.pc < seq.len() {
        if r.steps == MAX_STEPS {
            return Err(Error::PcBreak);
        }
        let inst = seq[r.pc];
        if inst.is_zero() {
            break;
        }
        let (result, data) = step(inst)?;
        if let Some(d) = data {
            if codec::plw(d) {
                payload = d;
            }
        }
        r.steps += 1;
        let code = low_u64(field(inst, 250, 3)) as u8;
        if !control_flow(&mut r, code, field(inst, 18, 232), result)? {
            break;
        }
    }
    if !r.ac {
        return Err(Error::AluBreak);
    }
    Ok(payload)
}

// ---- encoding checks ---------------------------------------------------------------------------------------------

pub fn verify_encoding(inst: U256) -> Result<(), Error> {
    let op = codec::opcode(inst);
    let dest = codec::rd(inst);
    let slot = codec::subr(inst);
    let bad = Err(Error::EncBreak);

    if op == opcode::TOP {
        if dest != rd::STD {
            return Ok(()); // extensions validate their own fields
        }
        if !(inst >> 251usize).is_zero() || slot != 0 {
            return bad;
        }
        let d = codec::decode_dcb(inst)?;
        verify_primary(d.rs1, d.funct4_1, d.imm_1)?;
        if d.rs2 == rs1::NUL {
            if d.funct4_2 != funct4::NOP || d.imm_2 != 0 || d.gate != 0 {
                return bad;
            }
        } else {
            verify_secondary(d.rs2, d.funct4_2, d.imm_2)?;
        }
        Ok(())
    } else if op == opcode::BOP || op == opcode::TXOP {
        if dest != rd::STD {
            return Ok(());
        }
        if !(inst >> 253usize).is_zero() {
            return bad;
        }
        let c = codec::decode_cb(inst)?;
        if c.ctl >= ctl::TRAP || slot != 0 {
            return bad;
        }
        verify_cb(&c, op == opcode::TXOP)
    } else {
        bad
    }
}

fn verify_bounds(imm: U256) -> Result<(), Error> {
    if !(imm >> 144usize).is_zero() || field(imm, 0, 72) > field(imm, 72, 72) {
        return Err(Error::EncBreak);
    }
    Ok(())
}

fn verify_primary(r1: u8, f4: u8, imm1: U256) -> Result<(), Error> {
    if f4 >= funct4::TRAP {
        return Err(Error::EncBreak);
    }
    if f4 == funct4::NOP {
        return if r1 != rs1::NUL || !imm1.is_zero() { Err(Error::EncBreak) } else { Ok(()) };
    }
    if r1 == rs1::NUL || r1 >= rs1::TRAP {
        return Err(Error::EncBreak);
    }
    match f4 {
        funct4::BEQZ | funct4::BNEZ if !imm1.is_zero() => Err(Error::EncBreak),
        funct4::BINB | funct4::BOOB => verify_bounds(imm1),
        _ => Ok(()),
    }
}

fn verify_secondary(r2: u8, f4: u8, imm2: u64) -> Result<(), Error> {
    if f4 == funct4::NOP || f4 >= funct4::BINB || r2 >= rs1::TRAP {
        return Err(Error::EncBreak);
    }
    let zero_check = f4 == funct4::BEQZ || f4 == funct4::BNEZ;
    // 160-bit addresses do not fit the 64-bit second immediate, except for zero checks
    if (r2 == rs1::SNDR || r2 == rs1::RCPT) && !zero_check {
        return Err(Error::EncBreak);
    }
    if zero_check && imm2 != 0 {
        return Err(Error::EncBreak);
    }
    Ok(())
}

fn verify_cb(c: &codec::Cb, is_txn: bool) -> Result<(), Error> {
    let bad = Err(Error::EncBreak);
    if c.funct4 >= funct4::TRAP {
        return bad;
    }
    let is_jump = c.ctl >= ctl::JMP;
    if c.funct4 == funct4::NOP {
        if c.rs1 != rs1::NUL || (!is_jump && !c.imm.is_zero()) {
            return bad;
        }
    } else {
        // transaction-level sequences see no single token, so they may only read sender, recipient, amount, clock…
        if c.rs1 == rs1::NUL || c.rs1 >= rs1::TRAP || (is_txn && c.rs1 < rs1::SNDR) {
            return bad;
        }
        if !is_jump {
            match c.funct4 {
                funct4::BEQZ | funct4::BNEZ if !c.imm.is_zero() => return bad,
                funct4::BINB | funct4::BOOB => verify_bounds(c.imm)?,
                _ => {}
            }
        }
    }
    if is_jump {
        if c.imm.is_zero() || c.imm >= U256::from(MAX_SEQUENCE_SIZE) {
            return bad;
        }
        if c.ctl == ctl::JMP {
            if c.funct4 != funct4::NOP {
                return bad;
            }
        } else if c.funct4 != funct4::NOP && c.funct4 != funct4::BEQZ && c.funct4 != funct4::BNEZ {
            return bad;
        }
    }
    Ok(())
}
