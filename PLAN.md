# zvm: moneyvm for shielded Zcash (working name)

ZECATHON entry, due 28 Oct 2026. Built on moneyvm by Sirawit Techavanitch (Apache-2.0, used with the author's permission).

## The idea

Programmable money for shielded ZEC. Each unit of deposited ZEC carries a 256-bit moneyvm instruction that runs on every
transfer: expiry, time locks, escrow with an arbiter, hash time-locked claims, allow/deny rules on sender, recipient and
amount. Zcash has no smart contracts, so the VM runs off-chain as a deterministic state machine, and Zcash carries the
money in and out.

## Architecture

```
 shielded ZEC + memo "zvm1:dep:<owner>"            signed commands (HTTP)
            │                                                │
            ▼                                                ▼
   ┌──────────────── wallet (Rust, from ZLOOP) ──┐    ┌──── sequencer ────┐
   │ watches the vault address, reads memos      │──▶ │ orders deposits + │
   │ pays withdrawals as shielded ZEC            │◀── │ commands, appends │
   └─────────────────────────────────────────────┘    │ to the public log │
                                                      └─────────┬─────────┘
                                                                ▼
                                          zvm-core (Rust, pure + deterministic)
                                          Codec · ISA · CoprocessorVM · Ledger
                                          extensions: escrow, HTLC
                                                                │
                                         state root every batch ▼
                                          anchored in a shielded memo on Zcash
```

- **zvm-core**: a faithful Rust port of moneyvm. No I/O, no clock of its own: block time and height come in with each
  command, so anyone can replay the log and get the same state. Pure Rust also lets us prove execution in a zkVM later.
- **Owners** are 160-bit ids derived from an ed25519 public key (same width as the EVM address the ISA expects). Users
  sign commands in the browser.
- **Money in**: send shielded ZEC to the vault with a memo naming the owner; the VM mints a unit for the exact amount.
- **Money out**: the unit owner signs a withdraw command; the vault pays shielded ZEC to their address.
- **Verifiability**: the command log is public and each batch's state root is written into a memo on Zcash, so anyone
  can replay and check the operator. Custody is still the operator's in v1 (roadmap: FROST threshold custody).

## Why not NEAR (for v1)

A NEAR contract can only control transparent ZEC (chain signatures sign the transparent key type, not Orchard's). That
drops the privacy, which is what Zcash judges weigh most. Kept as a roadmap item for cross-chain settlement.

## Build order

1. zvm-core port + tests (codec round-trips, ALU, control flow, ledger, escrow, HTLC, moneyvm's two scenarios).
2. Sequencer + log + replay + state root.
3. Wallet glue: deposits from memos, withdrawals as payments. Gate 1 on mainnet (needs funding).
4. Web app: deposit QR, units with their rules, send, escrow, HTLC, withdraw, replay/audit page.
5. README, video, submission.

## Upstream issues found while porting (to report to the moneyvm author)

- `CoprocessorVM.warp_rs` case 7 (`pl`) uses `add` where `and` is meant.
- `Ledger.reallocate` expiry check compares `timestamp > timestamp`, so expired tokens still move.
- Extension calls: `forward_single` encodes `(sender, recipient, token, amount, instruction)` but never encodes the
  `uint32` payload its selector declares, and `XUnit.execute` takes `(token, instruction, amount, payload)`. The
  single-instruction `step` passes `instruction, amount` swapped, which cancels one swap; the sequence `step` does not.
  The port defines one order: `(sender, recipient, token, amount, instruction, payload)`.
- Register numbers: `ISA.sol` has `tmd = 4, iat = 5, ts = 6`, but `warp_rs` reads `4 → iat, 5 → ts, 6 → tmd`. The port
  follows `ISA.sol`.
- Two splits in the same block can serialize to the same token id, and the second overwrites the first. The port moves
  to the next free slice. (More likely here: many commands share one 75 s Zcash block time.)
- `burn` has no owner check, and `attachInstruction` / `detachInstruction` have no access control in moneyvm. The port
  allows only the owner to attach, rules are one-way (no detach), and withdrawals run every rule (transfer to `EXIT`).
- Question for the author: a moved token is re-serialized with `tmd = 0, ts = 0`, so an expiry or time lock does not
  follow the money after its first transfer. Intended? The port keeps moneyvm's behaviour for now.

## Status

- 22 Oct: zvm-core ported, 22 tests pass (`cargo test`), including moneyvm's freeze constant byte for byte and a
  scaled-down version of its scenario 2 (freezing tainted branches leaves clean money alone).
- 22 Oct: sequencer (`zvm/node`, binary `zvm`): signed ed25519 commands with nonces, append-only `log.jsonl` with the
  block time, height and root of every entry, `zvm replay` that rebuilds and checks every root, HTTP API
  (`/api/info`, `/api/log`, `/api/owner/{id}`, `/api/whoami/{pubkey}`, `/api/cmd`, and `/api/dev/deposit` on the
  stand-in chain). 24 tests pass. Real-chain wallet not wired in yet (`zvm serve` requires `--fake`).
- 23 Oct: vault wallet wired in (`zvm/wallet`, copied from ZLOOP and extended with block time and chain tip). The vault
  loop syncs every 20 s, runs the VM on the newest block's time and height, credits deposits with 2 confirmations whose
  memo is `zvm1:dep:<owner>`, pays withdrawals (fee 15,000 zats, memo `zvm1:out:<index>`), and anchors the root every
  30 min (`zvm1:root:<index>:<root>`). CLI: `wallet-init`, `wallet-info`, `keygen`, `submit`. Mainnet vault created
  in `~/.zvm/vault` (seed there, never printed) and synced to block 3,526,898. 28 tests pass. Waiting on funding for
  Gate 1.
- 23 Oct: web app (`zvm/web`, no framework, served by `zvm serve`): wallet, deposit (ZIP-321 QR with the memo), send,
  program money (5 templates with the 256-bit instruction drawn as a bit field), escrow, hash lock, withdraw, audit,
  key backup. Ed25519 keys via WebCrypto in the browser. Playwright walkthrough on the stand-in chain passes with three
  users (deposit, send, issuer cap refusing two holders, escrow release, HTLC claim, withdraw), no phone overflow.
- 23 Oct (evening): user rejected the light UI; redesigned as a dark wallet-style app (balance hero, token cards with
  rule badges and glowing instruction strips, icon rule templates). `e2e/walk.mjs` (Playwright, 3 users, 13 checks)
  passes. Mainnet node restarted (log at `~/.zvm/serve.log`), synced to 3,527,991; no deposit has arrived yet.

