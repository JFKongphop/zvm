//! zvm-core: moneyvm's programmable-money VM as a pure Rust state machine.
//!
//! Based on moneyvm by Sirawit Techavanitch (Apache-2.0), used with the author's permission.
//! Nothing here reads a clock or does I/O: every command carries the Zcash block time and height that ordered it,
//! so replaying the same command log always gives the same state and the same state root.

pub mod codec;
pub mod ext;
pub mod isa;
pub mod ledger;
pub mod machine;
pub mod vm;

pub use machine::Machine;
pub use ruint::aliases::U256;

/// A 160-bit owner id, the width the instruction set reserves for addresses.
pub type Addr = [u8; 20];

/// Where withdrawals go: leaving the VM is a transfer to this id, so every rule on the money still applies.
pub const EXIT: Addr = [0xEE; 20];

pub fn addr_word(a: &Addr) -> U256 {
    U256::from_be_slice(a)
}

/// The Zcash context a command executes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Env {
    /// block time of the Zcash block that ordered the command (seconds)
    pub time: u64,
    pub height: u64,
    pub network_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidInstructionFormat,
    InvalidTokenExpiration,
    /// an instruction is not well formed
    EncBreak,
    /// a rule evaluated to false
    AluBreak,
    /// an extension refused the transfer
    EBreak { rd: u8, subr: u8 },
    CtlBreak(u8),
    /// the sequence ran out of steps
    PcBreak,
    /// no extension in that slot
    RtgBreak,
    JmpBreak,
    InvalidArrayLength,
    InvalidToken(u8),
    InsufficientBalance,
    SameSequence,
    Unauthorized,
    AlreadySet,
    UnknownToken,
    InvalidParameters,
    CommitmentAlreadyConsumed,
    CommitmentNotConsumed,
    UnauthorizedArbiter,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Reasons attached to `Error::InvalidToken`, numbered as in moneyvm's Ledger.
pub mod token_error {
    pub const WITH_TIMESTAMP: u8 = 1;
    pub const WITH_INSTRUCTION: u8 = 2;
    pub const UNAUTHORIZED: u8 = 3;
    pub const LOCKED: u8 = 4;
    pub const EXPIRED: u8 = 5;
}
