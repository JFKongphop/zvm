use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use zvm_core::Env;
use zvm_node::cmd::{hex_addr, owner_of};
use zvm_node::seq::{self, Config, Sequencer};

struct User {
    key: SigningKey,
    nonce: u64,
}

impl User {
    fn new(seed: u8) -> Self {
        User { key: SigningKey::from_bytes(&[seed; 32]), nonce: 0 }
    }
    fn id(&self) -> String {
        hex_addr(&owner_of(&self.key.verifying_key().to_bytes()))
    }
    /// Signs the exact JSON text, as the browser will.
    fn sign(&mut self, cmd: Value) -> (String, String) {
        self.nonce += 1;
        let payload = json!({ "net": 1, "nonce": self.nonce, "signer": hex::encode(self.key.verifying_key().to_bytes()), "cmd": cmd }).to_string();
        let sig = hex::encode(self.key.sign(payload.as_bytes()).to_bytes());
        (payload, sig)
    }
}

fn at(time: u64) -> Env {
    Env { time, height: time / 75, network_id: 1 }
}

fn cfg(admin: &User) -> Config {
    Config { network_id: 1, host: format!("0x{}", "5a".repeat(20)), admin: admin.id() }
}

#[test]
fn signed_commands_run_and_the_log_replays() {
    let dir = std::env::temp_dir().join(format!("zvm-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("log.jsonl");
    let _ = std::fs::remove_file(&file);

    let admin = User::new(9);
    let mut alice = User::new(1);
    let bob = User::new(2);
    let mut s = Sequencer::open(cfg(&admin), Some(file.clone())).unwrap();

    let dep = s.deposit(at(100), "tx-a".into(), zvm_node::cmd::parse_addr(&alice.id()).unwrap(), 100_000, 0, 0, 0).unwrap();
    let token = dep.result["token"].as_str().unwrap().to_string();
    assert!(s.deposit(at(100), "tx-a".into(), [1; 20], 5, 0, 0, 0).is_err(), "a deposit is credited once");

    let (p, sig) = alice.sign(json!({ "op": "transfer", "to": bob.id(), "tokens": [token], "amounts": [30_000] }));
    let moved = s.command(at(175), p.clone(), sig.clone()).unwrap();
    assert_eq!(s.holdings(&zvm_node::cmd::parse_addr(&bob.id()).unwrap())["zats"], 30_000);

    // the same signed command again is refused (nonce)
    assert!(s.command(at(250), p, sig).unwrap_err().contains("nonce"));
    // a tampered payload is refused
    let (p2, sig2) = alice.sign(json!({ "op": "transfer", "to": bob.id(), "tokens": [token], "amounts": [1] }));
    assert!(s.command(at(250), p2.replace("\"amounts\":[1]", "\"amounts\":[9]"), sig2).unwrap_err().contains("signature"));
    alice.nonce -= 1; // that one never landed

    // a refused command leaves no trace
    let before = s.state.root();
    let (p3, sig3) = alice.sign(json!({ "op": "transfer", "to": bob.id(), "tokens": [token], "amounts": [999_999_999] }));
    assert!(s.command(at(250), p3, sig3).is_err());
    assert_eq!(s.state.root(), before);
    alice.nonce -= 1;

    let (p4, sig4) = alice.sign(json!({ "op": "withdraw", "tokens": [token], "amounts": [20_000], "to": "u1example" }));
    let w = s.command(at(325), p4, sig4).unwrap();
    assert_eq!(w.result["payout"]["zats"], 20_000);
    assert_eq!(s.state.machine.ledger.total_supply, 80_000);
    let _ = moved;

    // a fresh sequencer rebuilds the same state from the file alone
    let root = s.state.root();
    let again = Sequencer::open(cfg(&admin), Some(file.clone())).unwrap();
    assert_eq!(again.state.root(), root);
    assert_eq!(again.log.len(), 3);

    // and a doctored log is caught
    let mut log = seq::read_log(&file).unwrap();
    log[1].time += 1;
    assert!(seq::replay(&cfg(&admin), &log).is_err(), "a changed timestamp changes the new token id, so it is caught");
    let mut log = seq::read_log(&file).unwrap();
    if let seq::Body::Deposit { zats, .. } = &mut log[0].body {
        *zats += 1;
    }
    let Err(err) = seq::replay(&cfg(&admin), &log) else { panic!("doctored log replayed") };
    assert_eq!(err.0, 0, "a changed amount fails at the entry that was changed");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn escrow_through_signed_commands() {
    let admin = User::new(9);
    let mut alice = User::new(1);
    let mut bob = User::new(2);
    let mut carol = User::new(3);
    let mut s = Sequencer::open(cfg(&admin), None).unwrap();
    let t = s.deposit(at(100), "tx".into(), zvm_node::cmd::parse_addr(&alice.id()).unwrap(), 50_000, 0, 0, 0).unwrap().result["token"].as_str().unwrap().to_string();

    let (p, sig) = alice.sign(json!({ "op": "escrow_lock", "token": t, "recipient": bob.id(), "arbiter": carol.id(), "salt": "11".repeat(32), "expiry": 10_000 }));
    let c = s.command(at(175), p, sig).unwrap().result["commitment"].as_str().unwrap().to_string();

    let (p, sig) = bob.sign(json!({ "op": "escrow_release", "commitment": c, "token": t }));
    assert!(s.command(at(250), p, sig).unwrap_err().contains("UnauthorizedArbiter"));
    bob.nonce -= 1;

    let (p, sig) = carol.sign(json!({ "op": "escrow_release", "commitment": c, "token": t }));
    s.command(at(250), p, sig).unwrap();
    assert_eq!(s.holdings(&zvm_node::cmd::parse_addr(&bob.id()).unwrap())["zats"], 50_000);
}
