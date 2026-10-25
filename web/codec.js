// moneyvm instruction encoding in the browser (mirrors zvm-core/src/codec.rs), plus keys and hashes.
'use strict';

const ISA = {
  op: { TOP: 0, BOP: 1, TXOP: 2 },
  f4: { NOP: 0, BEQ: 1, NEQ: 2, BLTU: 3, BLEU: 4, BGTU: 5, BGEU: 6, BEQZ: 7, BNEZ: 8, BINB: 9, BOOB: 10 },
  rs: { NUL: 0, ID: 1, SEQ: 2, SLC: 3, TMD: 4, IAT: 5, TS: 6, PL: 7, SNDR: 8, RCPT: 9, AMT: 10, CLK: 11, EPOCH: 12, NET: 13 },
  ctl: { END: 0, AND: 1, OR: 2, XOR: 3, REQ: 4, JMP: 5, JMPT: 6, JMPF: 7 },
};

const mask = (bits) => (1n << BigInt(bits)) - 1n;
const big = (x) => (typeof x === 'bigint' ? x : BigInt(x));

/** 2CB-type: a single instruction attached to one token. */
function encodeDcb({ rd = 0, subr = 0, imm1 = 0n, f4_1 = 0, rs1 = 0, rs2 = 0, f4_2 = 0, imm2 = 0n, gate = 0 }) {
  return (big(rd & 15) << 2n) | (big(subr & 15) << 6n) | ((big(imm1) & mask(160)) << 10n) |
    (big(f4_1 & 15) << 170n) | (big(rs1 & 15) << 174n) | (big(rs2 & 15) << 178n) |
    (big(f4_2 & 15) << 182n) | ((big(imm2) & mask(64)) << 186n) | (big(gate & 1) << 250n);
}

/** CB-type: one step of a base (1) or transaction (2) sequence. */
function encodeCb({ op, rd = 0, subr = 0, f4 = 0, rs1 = 0, imm = 0n, ctl = 0 }) {
  return big(op & 3) | (big(rd & 15) << 2n) | (big(subr & 15) << 6n) | (big(f4 & 15) << 10n) |
    (big(rs1 & 15) << 14n) | ((big(imm) & mask(232)) << 18n) | (big(ctl & 7) << 250n);
}

const field = (w, at, bits) => (w >> BigInt(at)) & mask(bits);
const wordHex = (w) => '0x' + big(w).toString(16).padStart(64, '0');
const addrBig = (hex) => BigInt(hex);

/** Segments of a word, for drawing it. */
function segments(w) {
  const op = Number(field(w, 0, 2));
  if (op === 0) {
    return [
      ['opcode', 0, 2, '--f-op'], ['rd', 2, 4, '--f-rd'], ['subr', 6, 4, '--f-sub'], ['imm_1', 10, 160, '--f-imm1'],
      ['funct4_1', 170, 4, '--f-fun'], ['rs1', 174, 4, '--f-rs'], ['rs2', 178, 4, '--f-rs'], ['funct4_2', 182, 4, '--f-fun'],
      ['imm_2', 186, 64, '--f-imm2'], ['gate', 250, 1, '--f-gate'], ['reserved', 251, 5, '--f-rsvd'],
    ].map(([name, at, bits, c]) => ({ name, at, bits, c, v: field(w, at, bits) }));
  }
  return [
    ['opcode', 0, 2, '--f-op'], ['rd', 2, 4, '--f-rd'], ['subr', 6, 4, '--f-sub'], ['funct4', 10, 4, '--f-fun'],
    ['rs1', 14, 4, '--f-rs'], ['imm', 18, 232, '--f-imm1'], ['ctl', 250, 3, '--f-gate'], ['reserved', 253, 3, '--f-rsvd'],
  ].map(([name, at, bits, c]) => ({ name, at, bits, c, v: field(w, at, bits) }));
}

// ---- bytes ---------------------------------------------------------------------------------------------------------

const hexToBytes = (h) => Uint8Array.from((h.replace(/^0x/, '').match(/../g) || []).map((b) => parseInt(b, 16)));
const bytesToHex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const u256Bytes = (x) => hexToBytes(big(x).toString(16).padStart(64, '0'));
const concat = (...parts) => { const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0)); let i = 0; for (const p of parts) { out.set(p, i); i += p.length; } return out; };
const utf8 = (s) => new TextEncoder().encode(s);

/** Owner id = first 20 bytes of SHA3-256("zvm-owner-v1" || ed25519 public key). */
function ownerOf(pubHex) {
  const h = sha3_256.array(concat(utf8('zvm-owner-v1'), hexToBytes(pubHex)));
  return '0x' + bytesToHex(h.slice(0, 20));
}

/** moneyvm's HTLC commitment: low 160 bits of keccak256(packed(net, host, sender, recipient, token, amount, preimage)). */
function htlcCommitment(net, host, sender, recipient, tokenHex, zats, preimageHex) {
  const packed = concat(u256Bytes(net), hexToBytes(host), hexToBytes(sender), hexToBytes(recipient), u256Bytes(BigInt(tokenHex)), u256Bytes(zats), hexToBytes(preimageHex));
  const h = keccak256.array(packed);
  return '0x' + bytesToHex(h.slice(12)).padStart(64, '0');
}

// ---- ed25519 keys (WebCrypto) -------------------------------------------------------------------------------------

const PKCS8_PREFIX = hexToBytes('302e020100300506032b657004220420');

async function keyFromSeed(seedHex) {
  const priv = await crypto.subtle.importKey('pkcs8', concat(PKCS8_PREFIX, hexToBytes(seedHex)), { name: 'Ed25519' }, true, ['sign']);
  const jwk = await crypto.subtle.exportKey('jwk', priv);
  const pub = bytesToHex(Uint8Array.from(atob(jwk.x.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0)));
  return { priv, pub, seed: seedHex, owner: ownerOf(pub) };
}

async function signText(priv, text) {
  return bytesToHex(new Uint8Array(await crypto.subtle.sign({ name: 'Ed25519' }, priv, utf8(text))));
}

function randomHex(n) {
  return bytesToHex(crypto.getRandomValues(new Uint8Array(n)));
}

const zec = (zats) => (Number(zats) / 1e8).toFixed(8).replace(/\.?0+$/, '');
const toZats = (s) => Math.round(parseFloat(s) * 1e8);
const short = (h, a = 6, b = 4) => (h && h.length > a + b + 3 ? `${h.slice(0, a + 2)}…${h.slice(-b)}` : h);
