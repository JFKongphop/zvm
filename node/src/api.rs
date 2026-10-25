//! HTTP API for the web app and for anyone auditing the vault.

use crate::cmd::{self, hex_addr, hex_word};
use zvm_core::codec;
use crate::seq::{Entry, Sequencer};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Mutex;
use zvm_core::Env;

/// Where the current Zcash block time and height come from.
pub enum Tip {
    /// stand-in chain for local work: wall-clock time, 75 s blocks
    Fake,
    /// set by the wallet after each sync
    Chain(Env),
}

pub struct Node {
    pub seq: Sequencer,
    pub tip: Tip,
    pub vault: Option<VaultInfo>,
    pub book: crate::vault::Book,
}

/// Public facts about the vault wallet.
pub struct VaultInfo {
    pub address: String,
    pub viewing_key_fingerprint: String,
    /// shown only when the operator chose to publish it (it reveals every deposit and payout)
    pub viewing_key: Option<String>,
    pub wallet: zvm_wallet::Wallet,
}

impl Node {
    pub fn env(&self) -> Env {
        match self.tip {
            Tip::Fake => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                Env { time: now, height: (now - 1_477_641_360) / 75, network_id: self.seq.cfg.network_id }
            }
            Tip::Chain(env) => env,
        }
    }
}

pub type Shared = Arc<Mutex<Node>>;

fn bad(msg: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.into() }))).into_response()
}

fn entry(e: Entry) -> Response {
    Json(serde_json::to_value(e).unwrap()).into_response()
}

async fn info(State(s): State<Shared>) -> Json<Value> {
    let n = s.lock().await;
    let env = n.env();
    Json(json!({
        "network_id": n.seq.cfg.network_id,
        "host": n.seq.cfg.host,
        "admin": n.seq.cfg.admin,
        "vault_address": n.vault.as_ref().map(|v| v.address.clone()),
        "viewing_key_fingerprint": n.vault.as_ref().map(|v| v.viewing_key_fingerprint.clone()),
        "viewing_key": n.vault.as_ref().and_then(|v| v.viewing_key.clone()),
        "withdraw_fee_zats": crate::vault::WITHDRAW_FEE,
        "deposit_confirmations": crate::vault::CONFIRMATIONS,
        "fake_chain": matches!(n.tip, Tip::Fake),
        "tip": { "time": env.time, "height": env.height },
        "entries": n.seq.log.len(),
        "root": n.seq.state.root(),
        "supply_zats": n.seq.state.machine.ledger.total_supply as u64,
    }))
}

#[derive(Deserialize)]
struct Page {
    from: Option<usize>,
    limit: Option<usize>,
}

async fn log(State(s): State<Shared>, Query(p): Query<Page>) -> Json<Value> {
    let n = s.lock().await;
    let from = p.from.unwrap_or(0).min(n.seq.log.len());
    let to = (from + p.limit.unwrap_or(200).min(1000)).min(n.seq.log.len());
    Json(json!({ "total": n.seq.log.len(), "entries": n.seq.log[from..to] }))
}

async fn owner(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    match cmd::parse_addr(&id) {
        Ok(a) => Json(s.lock().await.seq.holdings(&a)).into_response(),
        Err(e) => bad(e),
    }
}

async fn whoami(Path(pubkey): Path<String>) -> Response {
    let Some(pk) = hex::decode(pubkey.trim_start_matches("0x")).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else {
        return bad("an ed25519 public key is 32 bytes of hex");
    };
    Json(json!({ "owner": hex_addr(&cmd::owner_of(&pk)) })).into_response()
}

#[derive(Deserialize)]
struct Submit {
    payload: String,
    sig: String,
}

async fn submit(State(s): State<Shared>, Json(b): Json<Submit>) -> Response {
    let mut n = s.lock().await;
    // a withdrawal burns VM money, so its Zcash address must be good before it runs
    if let Ok(signed) = serde_json::from_str::<cmd::Signed>(&b.payload) {
        if let cmd::Command::Withdraw { to, amounts, .. } = &signed.cmd {
            if amounts.iter().sum::<u64>() <= crate::vault::WITHDRAW_FEE {
                return bad(format!("a withdrawal must be above the {} zat fee", crate::vault::WITHDRAW_FEE));
            }
            if let Some(v) = &n.vault {
                if !v.wallet.accepts_memo(to) {
                    return bad("that is not a shielded Zcash address on this network");
                }
            }
        }
    }
    let env = n.env();
    match n.seq.command(env, b.payload, b.sig) {
        Ok(e) => entry(e),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct DevDeposit {
    owner: String,
    zats: u64,
    #[serde(default)]
    bound: u64,
    #[serde(default)]
    time_mode: u8,
    #[serde(default)]
    payload: u32,
}

/// Stand-in chain only: pretend shielded ZEC arrived.
async fn dev_deposit(State(s): State<Shared>, Json(b): Json<DevDeposit>) -> Response {
    let mut n = s.lock().await;
    if !matches!(n.tip, Tip::Fake) {
        return (StatusCode::NOT_FOUND, "not on a real chain").into_response();
    }
    let owner = match cmd::parse_addr(&b.owner) {
        Ok(a) => a,
        Err(e) => return bad(e),
    };
    let env = n.env();
    let txid = format!("fake-{}", n.seq.log.len());
    match n.seq.deposit(env, txid, owner, b.zats, b.bound, b.time_mode, b.payload) {
        Ok(e) => entry(e),
        Err(e) => bad(e),
    }
}

/// Escrows where `id` is the sender, recipient or arbiter, with the token that carries each one.
async fn escrows(State(s): State<Shared>, Path(id): Path<String>) -> Response {
    let me = match cmd::parse_addr(&id) {
        Ok(a) => a,
        Err(e) => return bad(e),
    };
    let n = s.lock().await;
    let m = &n.seq.state.machine;
    let list: Vec<Value> = m
        .escrow
        .locks
        .iter()
        .filter(|(_, e)| e.sender == me || e.recipient == me || e.arbiter == me)
        .map(|(c, e)| {
            // the token whose attached instruction routes to the escrow slot with this commitment
            let carrier = m.instructions.iter().find(|(_, i)| codec::rd(**i) == 1 && codec::imm_1(**i) == *c).and_then(|(cid, i)| {
                let token = *m.ledger.by_cid.get(cid)?;
                Some(json!({ "token": hex_word(token), "zats": m.ledger.tokens[&token].balance as u64, "expiry": codec::imm_2(*i), "mode": codec::subr(*i) }))
            });
            json!({
                "commitment": hex_word(*c),
                "sender": hex_addr(&e.sender), "recipient": hex_addr(&e.recipient), "arbiter": hex_addr(&e.arbiter),
                "stage": format!("{:?}", e.stage), "carrier": carrier,
            })
        })
        .collect();
    Json(json!({ "escrows": list })).into_response()
}

async fn book(State(s): State<Shared>) -> Json<Value> {
    Json(serde_json::to_value(&s.lock().await.book).unwrap())
}

/// The API plus the web app from `web_dir`.
pub fn app(state: Shared, web_dir: &std::path::Path) -> Router {
    router(state).fallback_service(tower_http::services::ServeDir::new(web_dir).append_index_html_on_directories(true))
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/vault", get(book))
        .route("/api/escrows/{id}", get(escrows))
        .route("/api/info", get(info))
        .route("/api/log", get(log))
        .route("/api/owner/{id}", get(owner))
        .route("/api/whoami/{pubkey}", get(whoami))
        .route("/api/cmd", post(submit))
        .route("/api/dev/deposit", post(dev_deposit))
        .with_state(state)
}
