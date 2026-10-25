use zvm_core::codec::{self, Cb, Dcb};
use zvm_core::ext::{Htlc, escrow_mode, htlc_mode};
use zvm_core::isa::{ctl, funct4, opcode, rs1, tmd};
use zvm_core::vm::Sequence;
use zvm_core::{Addr, EXIT, Env, Error, Machine, U256, addr_word, token_error};

const NET: u64 = 133; // any fixed network id
const HOST: Addr = [0x5A; 20];
const ADMIN: Addr = [0xAD; 20];
const ALICE: Addr = [0xA1; 20];
const BOB: Addr = [0xB0; 20];
const CAROL: Addr = [0xC0; 20];

fn at(time: u64) -> Env {
    Env { time, height: time / 75, network_id: 0 }
}

fn machine() -> Machine {
    Machine::new(NET, HOST, ADMIN)
}

/// "seq == 0" never holds (seq starts at 1), so attaching this freezes a token. From moneyvm's experiment test.
fn freeze() -> U256 {
    codec::encode_dcb(&Dcb { funct4_1: funct4::BEQ, rs1: rs1::SEQ, ..Default::default() }).unwrap()
}

fn cb(op: u8, f4: u8, r1: u8, imm: U256, control: u8) -> U256 {
    codec::encode_cb(&Cb { opcode: op, rd: 0, subr: 0, funct4: f4, rs1: r1, imm, ctl: control }).unwrap()
}

fn seq(steps: &[U256]) -> Sequence {
    let mut s: Sequence = Default::default();
    s[..steps.len()].copy_from_slice(steps);
    s
}

// ---- codec ---------------------------------------------------------------------------------------------------------

#[test]
fn freeze_instruction_matches_moneyvm_bytes() {
    let expected = U256::from_str_radix("0000000000000000000084000000000000000000000000000000000000000000", 16).unwrap();
    assert_eq!(freeze(), expected);
}

#[test]
fn dcb_and_cb_round_trip() {
    let d = Dcb { rd: 0, subr: 0, imm_1: U256::from(500u64), funct4_1: funct4::BLEU, rs1: rs1::AMT, rs2: rs1::CLK, funct4_2: funct4::BLTU, imm_2: 1_800_000_000, gate: 0 };
    let w = codec::encode_dcb(&d).unwrap();
    assert_eq!(codec::decode_dcb(w).unwrap(), d);

    let c = Cb { opcode: opcode::BOP, rd: 0, subr: 0, funct4: funct4::BGEU, rs1: rs1::AMT, imm: U256::from(7u8), ctl: ctl::AND };
    let w = codec::encode_cb(&c).unwrap();
    assert_eq!(codec::decode_cb(w).unwrap(), c);
}

#[test]
fn token_ids_round_trip() {
    let t = codec::serialize(42, 3, 9, tmd::TLK, 1_000, 2_000, 0x1234).unwrap();
    let ctx = codec::deserialize(t);
    assert_eq!((ctx.id, ctx.seq, ctx.slc, ctx.tmd, ctx.iat, ctx.ts, ctx.pl), (42, 3, 9, 1, 1_000, 2_000, 0x1234));
    assert_eq!(codec::cid(t), ctx.cid());
    // a time lock must unlock after it was issued
    assert_eq!(codec::serialize(1, 1, 1, tmd::TLK, 2_000, 1_000, 0), Err(Error::InvalidTokenExpiration));
}

#[test]
fn malformed_instructions_are_rejected() {
    // nop with a source register
    assert_eq!(codec::encode_dcb(&Dcb { rs1: rs1::AMT, ..Default::default() }), Err(Error::EncBreak));
    // bounds with min > max
    let bounds = U256::from(10u8) | U256::from(5u8) << 72usize;
    assert_eq!(codec::encode_dcb(&Dcb { funct4_1: funct4::BINB, rs1: rs1::AMT, imm_1: bounds, ..Default::default() }), Err(Error::EncBreak));
    // an address compared in the 64-bit second slot
    let d = Dcb { funct4_1: funct4::BNEZ, rs1: rs1::AMT, rs2: rs1::SNDR, funct4_2: funct4::BEQ, imm_2: 1, ..Default::default() };
    assert_eq!(codec::encode_dcb(&d), Err(Error::EncBreak));
    // a transaction rule cannot read token fields
    assert!(codec::encode_cb(&Cb { opcode: opcode::TXOP, funct4: funct4::BEQ, rs1: rs1::ID, imm: U256::from(1u8), ..Default::default() }).is_err());
    // jumps must land inside the sequence
    assert!(codec::encode_cb(&Cb { opcode: opcode::BOP, ctl: ctl::JMP, imm: U256::from(9u8), ..Default::default() }).is_err());
}

// ---- ledger --------------------------------------------------------------------------------------------------------

#[test]
fn partial_and_full_transfers_split_tokens() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    assert_eq!(m.ledger.total_supply, 1_000);

    let part = m.transfer(at(200), ALICE, BOB, &[t], &[300]).unwrap()[0];
    let p = codec::deserialize(part);
    assert_eq!((p.id, p.seq, p.slc), (1, 1, 2), "a partial move keeps the lineage and takes the next slice");
    assert_eq!(m.ledger.balance_of(&ALICE, t), 700);
    assert_eq!(m.ledger.balance_of(&BOB, part), 300);

    let whole = m.transfer(at(300), BOB, CAROL, &[part], &[300]).unwrap()[0];
    let w = codec::deserialize(whole);
    assert_eq!((w.id, w.seq, w.slc), (1, 2, 1), "a full move bumps seq");
    assert_eq!(m.ledger.total_supply, 1_000);
}

#[test]
fn splits_in_the_same_block_do_not_overwrite_each_other() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    let a = m.transfer(at(200), ALICE, BOB, &[t], &[100]).unwrap()[0];
    // Bob splits his slice in the same block: moneyvm would mint an id equal to `a` and overwrite it
    let b = m.transfer(at(200), BOB, CAROL, &[a], &[40]).unwrap()[0];
    assert_ne!(a, b);
    assert_eq!(m.ledger.balance_of(&BOB, a), 60);
    assert_eq!(m.ledger.balance_of(&CAROL, b), 40);
    assert_eq!(m.holdings(&ALICE).iter().map(|h| h.1).sum::<u128>() + 60 + 40, 1_000);
}

#[test]
fn only_the_owner_can_move_a_token() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    assert_eq!(m.transfer(at(200), BOB, BOB, &[t], &[1]), Err(Error::InvalidToken(token_error::UNAUTHORIZED)));
    assert_eq!(m.transfer(at(200), ALICE, BOB, &[t], &[1_001]), Err(Error::InsufficientBalance));
}

#[test]
fn time_locked_money_cannot_move_until_it_unlocks() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 500, 1_000, tmd::TLK, 0).unwrap();
    assert_eq!(m.transfer(at(999), ALICE, BOB, &[t], &[500]), Err(Error::InvalidToken(token_error::LOCKED)));
    assert!(m.transfer(at(1_000), ALICE, BOB, &[t], &[500]).is_ok());
}

#[test]
fn expired_money_cannot_move() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 500, 1_000, tmd::EXP, 0).unwrap();
    assert!(m.transfer(at(1_000), ALICE, BOB, &[t], &[100]).is_ok());
    let rest = m.holdings(&ALICE)[0].0;
    assert_eq!(rest, t);
    assert_eq!(m.transfer(at(1_001), ALICE, BOB, &[t], &[100]), Err(Error::InvalidToken(token_error::EXPIRED)));
}

// ---- rules ---------------------------------------------------------------------------------------------------------

#[test]
fn a_frozen_token_cannot_move_and_a_failed_command_changes_nothing() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    m.attach(ALICE, codec::cid(t), freeze()).unwrap();
    let root = m.state_root();
    assert_eq!(m.transfer(at(200), ALICE, BOB, &[t], &[1]), Err(Error::AluBreak));
    assert_eq!(m.state_root(), root);
    // and nobody can swap the rule for a looser one
    assert_eq!(m.attach(ALICE, codec::cid(t), U256::ZERO), Err(Error::AlreadySet));
}

#[test]
fn freezing_tainted_branches_leaves_clean_money_alone() {
    // moneyvm scenario 2, scaled down: illicit roots are layered through smurfs, dust lands next to clean funds,
    // then only the tainted slices are frozen.
    let mut m = machine();
    let roots: Vec<U256> = (0..5).map(|_| m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap()).collect();
    let mut active: Vec<(U256, Addr)> = roots.iter().map(|&r| (r, ALICE)).collect();
    for depth in 1..4u8 {
        for (r, slot) in active.iter_mut().enumerate() {
            let smurf = [depth * 16 + r as u8; 20];
            slot.0 = m.transfer(at(100 + depth as u64 * 75), slot.1, smurf, &[slot.0], &[100]).unwrap()[0];
            slot.1 = smurf;
        }
    }
    let mut leaves = vec![];
    for i in 0..10u8 {
        let leaf = [0xF0 | (i & 0x0F), 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, i];
        let clean = m.deposit(at(500), leaf, 1_000, 0, 0, 0).unwrap();
        let (tok, smurf) = active[i as usize % 5];
        let dust = m.transfer(at(600), smurf, leaf, &[tok], &[10]).unwrap()[0];
        leaves.push((leaf, clean, dust));
    }
    for &(leaf, _, dust) in &leaves {
        m.attach(leaf, codec::cid(dust), freeze()).unwrap();
    }
    for &(leaf, clean, dust) in &leaves {
        assert_eq!(m.transfer(at(700), leaf, BOB, &[dust], &[1]), Err(Error::AluBreak));
        assert!(m.transfer(at(700), leaf, BOB, &[clean], &[1]).is_ok(), "clean funds still move");
    }
}

#[test]
fn issuer_rules_follow_the_money() {
    // payroll money: no single transfer above 500
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 2_000, 0, 0, 0).unwrap();
    let id = codec::deserialize(t).id;
    let cap = cb(opcode::BOP, funct4::BLEU, rs1::AMT, U256::from(500u16), ctl::END);
    assert_eq!(m.set_base_sequence(BOB, id, seq(&[cap])), Err(Error::Unauthorized));
    m.set_base_sequence(ALICE, id, seq(&[cap])).unwrap();
    assert_eq!(m.set_base_sequence(ALICE, id, Default::default()), Err(Error::AlreadySet), "rules are fixed once set");

    assert_eq!(m.transfer(at(200), ALICE, BOB, &[t], &[600]), Err(Error::AluBreak));
    let bobs = m.transfer(at(200), ALICE, BOB, &[t], &[500]).unwrap()[0];
    // Bob received money of the same lineage, so the cap still applies to him
    assert!(m.transfer(at(300), BOB, CAROL, &[bobs], &[400]).is_ok());
}

#[test]
fn sequences_branch_with_or_and_jumps() {
    // allowed when amount <= 100 OR recipient == CAROL
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    let small = cb(opcode::BOP, funct4::BLEU, rs1::AMT, U256::from(100u8), ctl::AND);
    let to_carol = cb(opcode::BOP, funct4::BEQ, rs1::RCPT, addr_word(&CAROL), ctl::OR);
    // start with ac = true AND small, then OR to_carol: (small) || to_carol
    m.set_base_sequence(ALICE, 1, seq(&[small, to_carol])).unwrap();
    assert!(m.transfer(at(200), ALICE, BOB, &[t], &[50]).is_ok());
    assert!(m.transfer(at(200), ALICE, CAROL, &[t], &[500]).is_ok());
    assert_eq!(m.transfer(at(200), ALICE, BOB, &[t], &[200]), Err(Error::AluBreak));
}

#[test]
fn vault_wide_rules_block_a_recipient() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    let not_bob = cb(opcode::TXOP, funct4::NEQ, rs1::RCPT, addr_word(&BOB), ctl::END);
    assert_eq!(m.set_sequence(ALICE, 0, seq(&[not_bob])), Err(Error::Unauthorized));
    m.set_sequence(ADMIN, 0, seq(&[not_bob])).unwrap();
    assert_eq!(m.transfer(at(200), ALICE, BOB, &[t], &[1]), Err(Error::AluBreak));
    assert!(m.transfer(at(200), ALICE, CAROL, &[t], &[1]).is_ok());
}

// ---- withdrawals ---------------------------------------------------------------------------------------------------

#[test]
fn withdrawals_burn_and_obey_the_rules() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    assert_eq!(m.withdraw(at(200), ALICE, &[t], &[400]), Ok(400));
    assert_eq!(m.ledger.total_supply, 600);
    assert_eq!(m.transfer(at(200), ALICE, EXIT, &[t], &[1]), Err(Error::InvalidParameters), "EXIT is only reached by withdraw");

    let locked = m.deposit(at(100), BOB, 100, 5_000, tmd::TLK, 0).unwrap();
    assert_eq!(m.withdraw(at(200), BOB, &[locked], &[100]), Err(Error::InvalidToken(token_error::LOCKED)));
}

// ---- escrow --------------------------------------------------------------------------------------------------------

#[test]
fn escrow_release_by_the_arbiter() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
    let c = m.escrow_lock(ALICE, t, BOB, CAROL, [7; 32], 10_000, escrow_mode::STD).unwrap();

    assert!(matches!(m.transfer(at(200), ALICE, BOB, &[t], &[1_000]), Err(Error::EBreak { rd: 1, .. })), "locked");
    assert!(m.withdraw(at(200), ALICE, &[t], &[1_000]).is_err(), "cannot escape the escrow by withdrawing");
    assert_eq!(m.escrow_release(at(300), BOB, c, t), Err(Error::UnauthorizedArbiter));

    let paid = m.escrow_release(at(300), CAROL, c, t).unwrap();
    assert_eq!(m.ledger.balance_of(&BOB, paid), 1_000);
    assert_eq!(m.escrow_refund(at(400), CAROL, c, t), Err(Error::CommitmentAlreadyConsumed));
}

#[test]
fn escrow_refund_and_expiry_reclaim() {
    let mut m = machine();
    let a = m.deposit(at(100), ALICE, 300, 0, 0, 0).unwrap();
    let ca = m.escrow_lock(ALICE, a, BOB, CAROL, [1; 32], 10_000, escrow_mode::STD).unwrap();
    let back = m.escrow_refund(at(200), CAROL, ca, a).unwrap();
    assert_eq!(m.ledger.balance_of(&ALICE, back), 300);

    let b = m.deposit(at(100), ALICE, 200, 0, 0, 0).unwrap();
    m.escrow_lock(ALICE, b, BOB, CAROL, [2; 32], 1_000, escrow_mode::STD).unwrap();
    assert!(m.transfer(at(999), ALICE, ALICE, &[b], &[200]).is_err(), "not before expiry");
    let mine = m.transfer(at(1_000), ALICE, ALICE, &[b], &[200]).unwrap()[0];
    assert_eq!(m.ledger.balance_of(&ALICE, mine), 200);
}

#[test]
fn release_only_escrow_cannot_be_refunded() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 300, 0, 0, 0).unwrap();
    let c = m.escrow_lock(ALICE, t, BOB, CAROL, [3; 32], 10_000, escrow_mode::RELEASE_ONLY).unwrap();
    assert!(m.escrow_refund(at(200), CAROL, c, t).is_err());
    assert!(m.escrow_release(at(200), CAROL, c, t).is_ok());
}

// ---- HTLC ----------------------------------------------------------------------------------------------------------

#[test]
fn htlc_claim_with_the_preimage() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 800, 0, 0, 0).unwrap();
    let secret = [0x42; 32];
    let commitment = Htlc::commitment(NET, &HOST, &ALICE, &BOB, t, 800, &secret);
    m.htlc_lock(ALICE, t, commitment, 5_000, htlc_mode::STD).unwrap();

    assert!(m.htlc_claim(at(200), BOB, ALICE, t, [0x41; 32]).is_err(), "wrong preimage");
    assert!(m.htlc_claim(at(200), CAROL, ALICE, t, secret).is_err(), "right preimage, wrong claimant");
    assert!(m.transfer(at(200), ALICE, ALICE, &[t], &[800]).is_err(), "sender cannot reclaim early");
    let got = m.htlc_claim(at(200), BOB, ALICE, t, secret).unwrap();
    assert_eq!(m.ledger.balance_of(&BOB, got), 800);
}

#[test]
fn htlc_reclaim_after_expiry() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 800, 0, 0, 0).unwrap();
    let secret = [0x42; 32];
    let commitment = Htlc::commitment(NET, &HOST, &ALICE, &BOB, t, 800, &secret);
    m.htlc_lock(ALICE, t, commitment, 5_000, htlc_mode::STD).unwrap();
    assert!(m.htlc_claim(at(5_000), BOB, ALICE, t, secret).is_err(), "too late to claim");
    assert!(m.transfer(at(5_000), ALICE, ALICE, &[t], &[800]).is_ok());
}

#[test]
fn a_claim_on_money_without_an_htlc_fails() {
    let mut m = machine();
    let t = m.deposit(at(100), ALICE, 800, 0, 0, 0).unwrap();
    assert_eq!(m.htlc_claim(at(200), BOB, ALICE, t, [0; 32]), Err(Error::CommitmentNotConsumed));
    assert_eq!(m.ledger.balance_of(&ALICE, t), 800);
}

// ---- replay --------------------------------------------------------------------------------------------------------

#[test]
fn replaying_the_same_commands_gives_the_same_root() {
    let run = || {
        let mut m = machine();
        let t = m.deposit(at(100), ALICE, 1_000, 0, 0, 0).unwrap();
        let b = m.transfer(at(175), ALICE, BOB, &[t], &[250]).unwrap()[0];
        m.escrow_lock(BOB, b, CAROL, ALICE, [9; 32], 9_999, escrow_mode::STD).unwrap();
        let _ = m.transfer(at(250), BOB, ALICE, &[b], &[250]); // refused, and must leave no trace
        m.withdraw(at(325), ALICE, &[t], &[100]).unwrap();
        m.state_root()
    };
    assert_eq!(run(), run());
    assert_ne!(run(), machine().state_root());
}
