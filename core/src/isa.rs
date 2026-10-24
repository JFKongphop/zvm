//! Instruction set constants, ported from moneyvm `src/arch/ISA.sol`.
//! Copyright 2026 Sirawit Techavanitch (moneyvm), Apache-2.0. Rust port for zvm.

pub mod opcode {
    pub const TOP: u8 = 0x0;
    pub const BOP: u8 = 0x1;
    pub const TXOP: u8 = 0x2;
    pub const TRAP: u8 = 0x3;
}

/// Destination: 0 runs the internal ALU, 1..=15 route to an extension slot.
pub mod rd {
    pub const STD: u8 = 0x0;
    pub const TRAP: u8 = 0x10;
}

pub mod funct4 {
    pub const NOP: u8 = 0x0;
    pub const BEQ: u8 = 0x1;
    pub const NEQ: u8 = 0x2;
    pub const BLTU: u8 = 0x3;
    pub const BLEU: u8 = 0x4;
    pub const BGTU: u8 = 0x5;
    pub const BGEU: u8 = 0x6;
    pub const BEQZ: u8 = 0x7;
    pub const BNEZ: u8 = 0x8;
    /// in bounds: lhs within [min, max], both 72-bit, packed min | max << 72
    pub const BINB: u8 = 0x9;
    /// out of bounds
    pub const BOOB: u8 = 0xA;
    pub const TRAP: u8 = 0x10;
}

/// Source registers the ALU can read.
pub mod rs1 {
    pub const NUL: u8 = 0x0;
    pub const ID: u8 = 0x1;
    pub const SEQ: u8 = 0x2;
    pub const SLC: u8 = 0x3;
    pub const TMD: u8 = 0x4;
    pub const IAT: u8 = 0x5;
    pub const TS: u8 = 0x6;
    pub const PL: u8 = 0x7;
    pub const SNDR: u8 = 0x8;
    pub const RCPT: u8 = 0x9;
    pub const AMT: u8 = 0xA;
    /// block time of the Zcash block that ordered the command
    pub const CLK: u8 = 0xB;
    /// Zcash block height
    pub const EPOCH: u8 = 0xC;
    pub const NETWORK_ID: u8 = 0xD;
    pub const TRAP: u8 = 0x10;
}

pub mod ctl {
    pub const END: u8 = 0x0;
    pub const AND: u8 = 0x1;
    pub const OR: u8 = 0x2;
    pub const XOR: u8 = 0x3;
    pub const REQ: u8 = 0x4;
    pub const JMP: u8 = 0x5;
    pub const JMPT: u8 = 0x6;
    pub const JMPF: u8 = 0x7;
    pub const TRAP: u8 = 0x8;
}

/// Time mode of a token: `EXP` makes `ts` an expiry, `TLK` makes it a time lock.
pub mod tmd {
    pub const EXP: u8 = 0x0;
    pub const TLK: u8 = 0x1;
    pub const TRAP: u8 = 0x2;
}
