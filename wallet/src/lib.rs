//! The vault wallet. Copied from ZLOOP's wallet (our own code, written for ZECATHON) and extended for zvm.

pub mod cache;
pub mod wallet;

pub use wallet::{Outgoing, Received, Wallet};
