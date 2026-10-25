// zvm web app: no framework. State lives in `S`; each section is a render function.
'use strict';

const S = { info: null, me: null, hold: null, view: 'wallet', pick: null, tpl: 'freeze' };
const $ = (sel) => document.querySelector(sel);

function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const k of kids.flat()) if (k != null && k !== false) el.append(k.nodeType ? k : document.createTextNode(String(k)));
  return el;
}

function toast(msg, ok = true) {
  document.querySelector('.toast')?.remove();
  const t = h('div', { class: `toast ${ok ? 'ok' : 'bad'}`, role: 'status' }, msg);
  document.body.append(t);
  setTimeout(() => t.remove(), ok ? 3500 : 7000);
}

async function api(path, body) {
  const r = await fetch(path, body ? { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) } : {});
  const j = await r.json().catch(() => ({ error: `HTTP ${r.status}` }));
  if (!r.ok) throw new Error(j.error || `HTTP ${r.status}`);
  return j;
}

function copy(text) {
  navigator.clipboard?.writeText(text).then(() => toast('Copied'), () => toast('Copy failed: select the text instead', false));
}
const copyBtn = (text) => h('button', { class: 'btn ghost small', onclick: () => copy(text), 'aria-label': 'Copy' }, 'Copy');

// ---- icons ----------------------------------------------------------------------------------------------------------

const ICON_PATHS = {
  lock: 'M7 11V8a5 5 0 0 1 10 0v3M5 11h14v10H5z',
  clock: 'M12 7v5l3 2M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z',
  shield: 'M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z',
  key: 'M15 7a4 4 0 1 1-3.9 5H3v3h3v3h3v-3h2.1A4 4 0 0 1 15 7z',
  check: 'M5 12l4 4 10-10',
  snow: 'M12 2v20M4.9 6.5l14.2 11M4.9 17.5l14.2-11',
  user: 'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',
  cap: 'M4 18h16M7 18V9M12 18V5M17 18v-6',
  users: 'M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2 20a7 7 0 0 1 14 0M17 4a3.5 3.5 0 0 1 0 7M22 20a7 7 0 0 0-4-6.3',
  arrow: 'M5 12h14M13 6l6 6-6 6',
  down: 'M12 4v14M6 12l6 6 6-6',
  up: 'M12 20V6M6 12l6-6 6 6',
  code: 'M8 7l-5 5 5 5M16 7l5 5-5 5',
};
function icon(name) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('fill', 'none'); svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2'); svg.setAttribute('stroke-linecap', 'round'); svg.setAttribute('stroke-linejoin', 'round'); svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path'); path.setAttribute('d', ICON_PATHS[name]); svg.append(path);
  return svg;
}

// ---- key ------------------------------------------------------------------------------------------------------------

async function loadKey() {
  let seed = null;
  try { seed = localStorage.getItem('zvm.seed'); } catch {}
  if (!seed) {
    seed = randomHex(32);
    try { localStorage.setItem('zvm.seed', seed); } catch {}
  }
  S.me = await keyFromSeed(seed);
}

/** Signs and submits one command; the nonce comes from the server's count of this owner's accepted commands. */
async function run(cmd, okMsg) {
  const view = S.view;
  try {
    const mine = await api(`/api/owner/${S.me.owner}`);
    const payload = JSON.stringify({ net: S.info.network_id, nonce: mine.nonce + 1, signer: S.me.pub, cmd });
    const sig = await signText(S.me.priv, payload);
    const entry = await api('/api/cmd', { payload, sig });
    toast(okMsg || `Done · log entry ${entry.index}`);
    // if the user has moved to another page meanwhile, do not redraw it: that would wipe what they are typing
    await refresh(S.view === view);
    return entry;
  } catch (e) {
    toast(explain(e.message), false);
    return null;
  }
}

/** VM errors are terse; say what they mean. */
function explain(m) {
  const map = [
    [/AluBreak/, 'A rule on this money refused the transfer.'],
    [/EBreak \{ rd: 1/, 'This money is held in escrow: only the arbiter can release or refund it (or you, after it expires).'],
    [/EBreak \{ rd: 2/, 'This money is locked by an HTLC: it moves only with the right secret (or back to the sender after expiry).'],
    [/InvalidToken\(4\)/, 'This money is time-locked and cannot move yet.'],
    [/InvalidToken\(5\)/, 'This money has expired and can no longer move.'],
    [/InvalidToken\(3\)/, 'You do not own that token.'],
    [/InsufficientBalance/, 'That token does not hold that much.'],
    [/AlreadySet/, 'That token already carries a rule. Rules cannot be replaced.'],
    [/UnauthorizedArbiter/, 'Only the arbiter can do that.'],
    [/CommitmentNotConsumed|CommitmentAlreadyConsumed/, 'The secret, token or amount does not match the lock.'],
    [/Unauthorized/, 'You are not allowed to do that.'],
  ];
  for (const [re, text] of map) if (re.test(m)) { console.warn('[zvm]', m); return text; }
  return m;
}

// ---- data -----------------------------------------------------------------------------------------------------------

async function refresh(rerender = true) {
  try {
    [S.info, S.hold] = await Promise.all([api('/api/info'), api(`/api/owner/${S.me.owner}`)]);
  } catch (e) {
    toast(`Cannot reach the vault: ${e.message}`, false);
  }
  renderTop();
  if (rerender) render();
}

/** Short fingerprint of the full token id: two splits can share id·seq·slice and differ only in issue time. */
const tag = (t) => sha3_256(t.token).slice(0, 4);
/** A banknote-style serial: the first 48 bits of the token id's hash, in groups of four. */
const serialOf = (hex) => sha3_256(hex).slice(0, 12).toUpperCase().match(/..../g).join(' ');
const serial = (t) => serialOf(t.token);

/** "0.025" → "Zero point zero two five", as written on a cheque. */
function inWords(v) {
  const ones = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen'];
  const tens = ['', '', 'twenty', 'thirty', 'forty', 'fifty', 'sixty', 'seventy', 'eighty', 'ninety'];
  const int = (n) => n < 20 ? ones[n] : n < 100 ? tens[Math.floor(n / 10)] + (n % 10 ? '-' + ones[n % 10] : '')
    : n < 1000 ? ones[Math.floor(n / 100)] + ' hundred' + (n % 100 ? ' and ' + int(n % 100) : '')
    : n < 1e6 ? int(Math.floor(n / 1000)) + ' thousand' + (n % 1000 ? ' ' + int(n % 1000) : '') : String(n);
  const m = String(v).trim().match(/^(\d*)(?:\.(\d{0,8}))?/);
  if (!m || (!m[1] && !m[2])) return '';
  const frac = (m[2] || '').replace(/0+$/, '');
  const out = int(Number(m[1] || 0)) + (frac ? ' point ' + [...frac].map((d) => ones[d]).join(' ') : '');
  return out[0].toUpperCase() + out.slice(1);
}
const tokenLabel = (t) => `#${t.id}·${t.seq}·${t.slc} ${tag(t)} · ${zec(t.zats)} ZEC`;
const when = (secs) => new Date(secs * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const dateToSecs = (v) => (v ? Math.floor(new Date(v).getTime() / 1000) : 0);

function tokenSelect(id, filter = () => true) {
  const toks = (S.hold?.tokens || []).filter(filter);
  return h('select', { id }, toks.length ? toks.map((t) => h('option', { value: t.token, selected: S.pick === t.token }, tokenLabel(t))) : h('option', { value: '' }, 'No tokens yet: deposit first'));
}
const tokenById = (hex) => (S.hold?.tokens || []).find((t) => t.token === hex);

/** What a token's rules are, as one kind for colouring the card. */
function kindOf(t) {
  if (t.rule) return t.rule.kind === 'rule' ? 'rule' : t.rule.kind;
  if (t.lineage_rules) return 'rule';
  if (t.ts) return 'time';
  return 'free';
}

function badges(t) {
  const out = [];
  if (t.rule?.kind === 'escrow') out.push(h('span', { class: 'badge escrow' }, icon('shield'), 'escrow'));
  else if (t.rule?.kind === 'htlc') out.push(h('span', { class: 'badge htlc' }, icon('key'), 'hash lock'));
  else if (t.rule) out.push(h('span', { class: 'badge rule' }, icon('code'), 'rule'));
  if (t.lineage_rules) out.push(h('span', { class: 'badge rule' }, icon('cap'), 'issuer rules'));
  if (t.ts) out.push(h('span', { class: 'badge time' }, icon('clock'), `${t.time_mode === 'unlock' ? 'unlocks' : 'expires'} ${new Date(t.ts * 1000).toLocaleDateString()}`));
  if (!out.length) out.push(h('span', { class: 'badge free' }, icon('check'), 'free to move'));
  return h('div', { class: 'badges' }, out);
}
const chips = badges;

// ---- engraving -----------------------------------------------------------------------------------------------------

/** Deterministic random numbers from an id, so a wallet or token always gets the same engraving. */
function rng(hex) {
  let x = BigInt('0x' + (String(hex).replace(/^0x/, '').slice(-15) || '1')) | 1n;
  return () => { x = (x * 6364136223846793005n + 1442695040888963407n) & 0xffffffffffffffffn; return Number(x >> 11n) / 2 ** 53; };
}
const svgBox = (cls, body, vb) => { const d = document.createElement('div'); d.className = cls; d.setAttribute('aria-hidden', 'true');
  d.innerHTML = `<svg viewBox="${vb}" preserveAspectRatio="none" fill="none" stroke="currentColor" stroke-width=".6">${body}</svg>`; return d; };
const pathOf = (pts) => 'M' + pts.map(([x, y]) => `${x.toFixed(1)} ${y.toFixed(1)}`).join('L');

/** Guilloche band: interfering sine waves, like the border of a banknote. */
function band(hex, w = 600, h = 36, n = 16) {
  const r = rng(hex), f1 = 0.018 + r() * 0.014, f2 = 0.004 + r() * 0.005, f3 = 0.05 + r() * 0.04, amp = h * 0.44;
  let body = '';
  for (let k = 0; k < n; k++) {
    const pts = [];
    for (let x = 0; x <= w; x += 3) pts.push([x, h / 2 + amp * Math.sin(x * f1 + (k * Math.PI) / n * 2) * Math.cos(x * f2 + k * 0.3) + 1.2 * Math.sin(x * f3 + k)]);
    body += `<path d="${pathOf(pts)}" opacity="${0.45 + 0.5 * (k % 2)}"/>`;
  }
  return svgBox('band', body, `0 0 ${w} ${h}`);
}

/** Guilloche rosette: rings whose radius ripples with the owner id. */
function rosette(hex, size = 400) {
  const r = rng(hex), c = size / 2;
  let body = '';
  for (let j = 0; j < 7; j++) {
    const R = 46 + j * 21, a = 6 + r() * 12, k = 9 + Math.floor(r() * 14) + j * 2;
    for (let q = 0; q < 3; q++) {
      const ph = (q * Math.PI) / k / 1.5 + r(), pts = [];
      for (let i = 0; i <= 900; i++) { const t = (i / 900) * Math.PI * 2, rr = R + a * Math.cos(k * t + ph); pts.push([c + rr * Math.cos(t), c + rr * Math.sin(t)]); }
      body += `<path d="${pathOf(pts)}Z" opacity="${0.35 + 0.22 * q}"/>`;
    }
  }
  body += `<circle cx="${c}" cy="${c}" r="34" stroke-width="1"/><circle cx="${c}" cy="${c}" r="190" stroke-width=".8"/>`;
  return svgBox('rosette', body, `0 0 ${size} ${size}`);
}

/** What a token's rules mean, in words: [label, text] rows. */
function terms(t) {
  const rows = [];
  const word = (hx) => BigInt(hx);
  if (t.rule?.kind === 'escrow') rows.push(['Held', `in escrow until ${when(Number(t.rule.expiry))}`]);
  else if (t.rule?.kind === 'htlc') rows.push(['Held', 'behind a hash lock']);
  else if (t.rule) {
    const w = word(t.rule.inst), f4 = Number(field(w, 170, 4)), rs = Number(field(w, 174, 4)), imm = field(w, 10, 160);
    rows.push(['Rule', f4 === ISA.f4.BEQ && rs === ISA.rs.SEQ ? 'Frozen: can never move'
      : f4 === ISA.f4.BEQ && rs === ISA.rs.RCPT ? `Only to ${short('0x' + imm.toString(16).padStart(40, '0'))}`
      : f4 === ISA.f4.BGEU && rs === ISA.rs.CLK ? `Not before ${when(Number(imm))}` : 'Custom instruction']);
  }
  const seq = (t.lineage_seq || []).map(word);
  if (seq.length) {
    const parts = seq.map((w) => ({ f4: Number(field(w, 10, 4)), rs: Number(field(w, 14, 4)), imm: field(w, 18, 232) }));
    const text = parts.length === 1 && parts[0].f4 === ISA.f4.BLEU && parts[0].rs === ISA.rs.AMT ? `No transfer above ${zec(parts[0].imm)} ZEC`
      : parts.every((x) => x.f4 === ISA.f4.BEQ && x.rs === ISA.rs.RCPT) ? `Only to ${parts.map((x) => short('0x' + x.imm.toString(16).padStart(40, '0'))).join(' or ')}`
      : `${parts.length} issuer instruction${parts.length > 1 ? 's' : ''}`;
    rows.push(['Issuer', text]);
  } else if (t.lineage_rules) rows.push(['Issuer', 'Issuer rules']);
  if (t.ts) rows.push([t.time_mode === 'unlock' ? 'Locked' : 'Expires', `${t.time_mode === 'unlock' ? 'until ' : ''}${when(t.ts)}`]);
  if (!rows.length) rows.push(['Rule', 'None: free to move']);
  return rows;
}

// ---- header + nav ---------------------------------------------------------------------------------------------------

const SECTIONS = [
  ['wallet', 'Wallet'], ['deposit', 'Deposit'], ['send', 'Send'], ['program', 'Program'],
  ['escrow', 'Escrow'], ['htlc', 'Hash lock'], ['withdraw', 'Withdraw'], ['audit', 'Audit'],
];

function renderTop() {
  const i = S.info;
  $('#top').replaceChildren(
    i ? h('span', { class: `net ${i.fake_chain ? 'fake' : ''}`, title: `state root ${i.root}` }, h('span', { class: 'dot' }),
      h('span', { class: 'lbl' }, `${i.fake_chain ? 'Stand-in chain' : i.network_id === 1 ? 'Mainnet' : 'Testnet'} · block №`), h('strong', {}, i.tip.height.toLocaleString())) : null,
    S.me ? h('button', { class: 'me', title: 'Your key', onclick: () => go('key') }, short(S.me.owner, 4, 4)) : null,
  );
  $('#nav').replaceChildren(...SECTIONS.map((s) => h('button', { 'aria-current': S.view === s[0] ? 'true' : 'false', onclick: () => go(s[0]) }, s[1])));
}

function go(view, pick) {
  S.view = view;
  if (pick !== undefined) S.pick = pick;
  try { history.replaceState(null, '', `#${view}`); } catch {}
  renderTop();
  render();
}

function render() {
  const views = { wallet, deposit, send, program, escrow, htlc, withdraw, audit, key };
  $('#main').replaceChildren(...[].concat((views[S.view] || wallet)()));
}

// ---- wallet ---------------------------------------------------------------------------------------------------------

function wallet() {
  const toks = S.hold?.tokens || [];
  const i = S.info;
  return [
    h('section', { class: 'hero' },
      rosette(S.me.owner),
      h('span', { class: 'corner tl' }, 'zvm · shielded zcash'),
      h('span', { class: 'corner bl' }, S.me.owner),
      h('div', { class: 'eyebrow' }, 'Balance held for you'),
      h('div', { class: 'balance' }, zec(S.hold?.zats || 0), h('small', {}, 'ZEC')),
      h('div', { class: 'idline' }, 'Owner №', h('code', {}, short(S.me.owner, 10, 8)), copyBtn(S.me.owner)),
      h('div', { class: 'actions' },
        h('button', { class: 'btn gold', onclick: () => go('deposit') }, 'Deposit'),
        h('button', { class: 'btn', onclick: () => go('send') }, 'Send'),
        h('button', { class: 'btn', onclick: () => go('program') }, 'Program'),
        h('button', { class: 'btn ghost', onclick: () => go('withdraw') }, 'Withdraw')),
    ),
    h('div', { class: 'section-head' },
      h('h2', { style: { margin: 0 } }, `Notes in your wallet · ${toks.length}`),
      i ? h('span', { class: 'hint', style: { margin: 0 } }, `The vault holds ${zec(i.supply_zats)} ZEC across ${i.entries} log entries`) : null),
    toks.length
      ? h('div', { class: 'tokens' }, toks.map((t) => h('article', { class: `tok k-${kindOf(t)}` },
        h('div', { class: 'note-top' }, h('span', { class: 'denom' }, `Note № ${t.id}·${t.seq}·${t.slc}`), h('span', { class: 'amt' }, zec(t.zats), h('small', {}, 'ZEC'))),
        band(t.token),
        h('dl', { class: 'terms' },
          terms(t).flatMap(([k, v], n) => [h('dt', {}, k), h('dd', { class: n === 0 && v !== 'None: free to move' ? 'rule' : '' }, v)]),
          h('dt', {}, 'Serial'), h('dd', { class: 'serial', title: t.token }, serial(t)),
          h('dt', {}, 'Issued'), h('dd', {}, when(t.iat))),
        h('div', { class: 'acts' },
          h('button', { onclick: () => go('send', t.token) }, 'Send'),
          h('button', { onclick: () => go('program', t.token) }, 'Program'),
          h('button', { onclick: () => go('withdraw', t.token) }, 'Withdraw')),
      )))
      : h('div', { class: 'card empty' }, h('span', { class: 'big' }, 'No notes yet'), 'Deposit shielded ZEC and it is issued to you here as programmable money. ',
        h('a', { href: '#deposit', onclick: (e) => { e.preventDefault(); go('deposit'); } }, 'Make a deposit')),
  ];
}

// ---- deposit --------------------------------------------------------------------------------------------------------

function b64url(s) {
  return btoa(String.fromCharCode(...utf8(s))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function qr(text) {
  try {
    const q = qrcode(0, 'M');
    q.addData(text, 'Byte');
    q.make();
    const n = q.getModuleCount(), sc = 4;
    const c = h('canvas', { class: 'qr', width: n * sc, height: n * sc, role: 'img', 'aria-label': 'Payment QR code' });
    const ctx = c.getContext('2d');
    ctx.fillStyle = '#000';
    for (let r = 0; r < n; r++) for (let col = 0; col < n; col++) if (q.isDark(r, col)) ctx.fillRect(col * sc, r * sc, sc, sc);
    return c;
  } catch {
    return h('p', { class: 'hint' }, 'The QR code could not be drawn: use the address and memo.');
  }
}

function deposit() {
  const stub = h('div', { class: 'slip-stub' });
  const lines = h('div', { class: 'slip-lines' });
  const words = h('div', { class: 'words' });
  const line = (k, v, copy) => h('div', { class: 'line' }, h('span', { class: 'k' }, k), h('span', { class: 'v' }, v), copy ? copyBtn(v) : h('span'));
  const draw = () => {
    const amt = $('#dep-amt').value, mode = $('#dep-mode').value, date = $('#dep-date').value;
    const bound = mode === 'none' ? 0 : dateToSecs(date);
    const memo = bound ? `zvm1:dep:${S.me.owner}:${bound}:${mode === 'unlock' ? 1 : 0}:0` : `zvm1:dep:${S.me.owner}`;
    const zats = toZats(amt || '0');
    words.replaceChildren(h('span', { class: 'k' }, 'The sum of'), zats ? `${inWords(amt)} ZEC` : '—');
    if (S.info.fake_chain) {
      lines.replaceChildren(line('Memo it would carry', memo));
      stub.replaceChildren(
        h('div', { class: 'stamp' }, 'Stand-in chain'),
        h('p', { class: 'hint', style: { margin: 0, textAlign: 'center' } }, 'This vault runs without Zcash. The button pretends the shielded payment arrived.'),
        h('button', { class: 'btn gold', disabled: !zats, onclick: async () => {
          try { await api('/api/dev/deposit', { owner: S.me.owner, zats, bound, time_mode: mode === 'unlock' ? 1 : 0, payload: 0 }); toast('Deposit credited'); await refresh(); go('wallet'); }
          catch (e) { toast(explain(e.message), false); }
        } }, `Simulate a ${zec(zats)} ZEC deposit`));
      return;
    }
    const addr = S.info.vault_address;
    lines.replaceChildren(line('Pay to the vault', addr, true), line('Memo · credits you', memo, true), line('Amount', `${zec(zats)} ZEC`));
    stub.replaceChildren(qr(`zcash:${addr}?amount=${zec(zats)}&memo=${b64url(memo)}`),
      h('div', { class: 'stub-cap' }, 'Scan with Zashi, Zingo or YWallet'),
      h('p', { class: 'hint', style: { margin: 0, textAlign: 'center' } }, `Credited after ${S.info.deposit_confirmations} confirmations. This page updates by itself.`));
  };
  queueMicrotask(draw);
  return [
    h('h1', {}, 'Deposit'),
    h('p', { class: 'lede' }, 'Send shielded ZEC to the vault. The memo names you, so the vault issues it to your owner id as one note. Send without the memo and the vault cannot tell the money is yours.'),
    h('section', { class: 'slip' },
      h('div', { class: 'slip-main' },
        h('div', { class: 'slip-head' },
          h('div', {}, h('div', { class: 'kicker' }, 'zvm vault · shielded Zcash'), h('div', { class: 'doc-title' }, 'Deposit slip')),
          h('div', { class: 'docno' }, h('span', { class: 'k' }, 'Owner No.'), h('span', { class: 'serial' }, serialOf(S.me.owner)))),
        h('div', { class: 'row' },
          h('label', {}, 'Amount (ZEC)', h('input', { id: 'dep-amt', type: 'number', min: '0', step: '0.001', value: '0.05', oninput: draw })),
          h('label', {}, 'Built-in time rule', h('select', { id: 'dep-mode', onchange: draw },
            h('option', { value: 'none' }, 'None'), h('option', { value: 'unlock' }, 'Locked until a date'), h('option', { value: 'expiry' }, 'Expires on a date'))),
          h('label', {}, 'Date', h('input', { id: 'dep-date', type: 'datetime-local', oninput: draw }))),
        words, lines,
        h('p', { class: 'hint' }, 'A time rule is written into the note itself when the money arrives: locked money cannot move before the date, expired money cannot move after it.')),
      stub),
  ];
}

// ---- send -----------------------------------------------------------------------------------------------------------

function send() {
  return [
    h('h1', {}, 'Send'),
    h('p', { class: 'lede' }, 'Move money to another owner id inside zvm. Every rule on the token runs first; if any refuses, nothing moves.'),
    h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('label', {}, 'Token', tokenSelect('s-tok')),
        h('label', {}, 'Amount (ZEC)', h('input', { id: 's-amt', type: 'number', step: '0.00000001', min: '0' })),
      ),
      h('div', { class: 'row', style: { marginTop: '12px' } },
        h('label', {}, 'Recipient owner id', h('input', { id: 's-to', placeholder: '0x… (40 hex)', autocomplete: 'off' })),
        h('button', { class: 'btn', onclick: () => run({ op: 'transfer', to: $('#s-to').value.trim(), tokens: [$('#s-tok').value], amounts: [toZats($('#s-amt').value)] }, 'Sent') }, 'Send'),
      ),
    ),
  ];
}

// ---- program --------------------------------------------------------------------------------------------------------

const TEMPLATES = {
  freeze: { ico: 'snow', name: 'Freeze', desc: 'This token can never move again.', scope: 'this token', fields: [] },
  only: { ico: 'user', name: 'Only to one person', desc: 'This token can only go to one owner id.', scope: 'this token', fields: [['who', 'Allowed recipient owner id', 'text']] },
  after: { ico: 'clock', name: 'Not before a date', desc: 'This token cannot move before a date.', scope: 'this token', fields: [['date', 'Earliest date', 'datetime-local']] },
  cap: { ico: 'cap', name: 'Spending cap', desc: 'No single transfer of this money above a limit, for whoever holds it.', scope: 'all money from this deposit', fields: [['cap', 'Cap per transfer (ZEC)', 'number']] },
  pair: { ico: 'users', name: 'Two allowed recipients', desc: 'This money can only ever go to one of two owner ids.', scope: 'all money from this deposit', fields: [['a', 'Recipient A', 'text'], ['b', 'Recipient B', 'text']] },
};

/** Builds the instruction(s) for a template from the form. Returns { kind: 'attach' | 'lineage', words }. */
function buildTemplate(tpl, v) {
  const { f4, rs, op, ctl } = ISA;
  switch (tpl) {
    case 'freeze': return { kind: 'attach', words: [encodeDcb({ f4_1: f4.BEQ, rs1: rs.SEQ, imm1: 0n })] }; // seq is never 0
    case 'only': return { kind: 'attach', words: [encodeDcb({ f4_1: f4.BEQ, rs1: rs.RCPT, imm1: addrBig(v.who) })] };
    case 'after': return { kind: 'attach', words: [encodeDcb({ f4_1: f4.BGEU, rs1: rs.CLK, imm1: BigInt(dateToSecs(v.date)) })] };
    case 'cap': return { kind: 'lineage', words: [encodeCb({ op: op.BOP, f4: f4.BLEU, rs1: rs.AMT, imm: BigInt(toZats(v.cap)), ctl: ctl.END })] };
    case 'pair': return { kind: 'lineage', words: [
      encodeCb({ op: op.BOP, f4: f4.BEQ, rs1: rs.RCPT, imm: addrBig(v.a), ctl: ctl.AND }),
      encodeCb({ op: op.BOP, f4: f4.BEQ, rs1: rs.RCPT, imm: addrBig(v.b), ctl: ctl.OR }),
    ] };
  }
}

function wordView(w) {
  const segs = segments(w);
  return h('div', { class: 'word' },
    h('div', { class: 'bits', role: 'img', 'aria-label': 'Instruction bit field' }, segs.map((s) => h('div', {
      class: s.v ? 'on' : '', title: `${s.name} [${s.at}:${s.at + s.bits - 1}] = 0x${s.v.toString(16)}`,
      style: { flex: `${s.bits} 0 0`, background: `var(${s.c})`, opacity: s.v ? 1 : 0.35 },
    }))),
    h('div', { class: 'legend' }, segs.filter((s) => s.name !== 'reserved').map((s) => h('span', { style: { '--c': `var(${s.c})` } }, `${s.name} ${s.v > 0xffffn ? '0x' + short(s.v.toString(16), 4, 4).replace(/^0x/, '') : s.v}`))),
    h('div', { class: 'hex' }, wordHex(w)),
  );
}

function program() {
  const tpl = TEMPLATES[S.tpl];
  const preview = h('div');
  const values = () => Object.fromEntries(tpl.fields.map(([k]) => [k, $(`#p-${k}`)?.value || '']));
  const draw = () => {
    let built;
    try { built = buildTemplate(S.tpl, values()); } catch { built = null; }
    preview.replaceChildren(...(built ? built.words.map(wordView) : [h('p', { class: 'hint' }, 'Fill in the fields to see the instruction.')]));
  };
  queueMicrotask(draw);
  const apply = async () => {
    const tok = tokenById($('#p-tok').value);
    if (!tok) return toast('Pick a token first', false);
    let built;
    try { built = buildTemplate(S.tpl, values()); } catch (e) { return toast(`Check the fields: ${e.message}`, false); }
    if (built.kind === 'attach') await run({ op: 'attach', token: tok.token, inst: wordHex(built.words[0]) }, 'Rule attached');
    else await run({ op: 'set_base_sequence', id: tok.id, seq: built.words.map(wordHex) }, 'Rules set for all money from that deposit');
  };
  return [
    h('h1', {}, 'Program money'),
    h('p', { class: 'lede' }, 'Attach a moneyvm instruction to a token. It runs on every transfer and can refuse it. Rules are one-way: once set, nobody can remove them, not even you.'),
    h('div', { class: 'templates' }, Object.entries(TEMPLATES).map(([k, t]) => h('button', { class: 'tpl', 'aria-pressed': S.tpl === k ? 'true' : 'false', onclick: () => { S.tpl = k; render(); } },
      h('div', { class: 'ico' }, icon(t.ico)), h('strong', {}, t.name), h('span', {}, t.desc), h('small', {}, t.scope)))),
    h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('label', {}, 'Token', tokenSelect('p-tok', (t) => !t.rule)),
        ...tpl.fields.map(([k, label, type]) => h('label', {}, label, h('input', { id: `p-${k}`, type, oninput: draw, step: type === 'number' ? '0.00000001' : null, placeholder: type === 'text' ? '0x…' : null }))),
        h('button', { class: 'btn gold', onclick: apply }, 'Apply rule'),
      ),
      tpl.scope.startsWith('all') ? h('p', { class: 'hint' }, 'Issuer rules belong to the deposit the token came from, so only whoever made that deposit can set them, and only once. They follow every piece of that money, whoever holds it.') : null,
      h('h2', { style: { marginTop: '18px' } }, 'The instruction'),
      preview,
    ),
  ];
}

// ---- escrow ---------------------------------------------------------------------------------------------------------

function escrow() {
  const list = h('div', {}, h('div', { class: 'empty' }, 'Loading…'));
  api(`/api/escrows/${S.me.owner}`).then(({ escrows }) => {
    if (!escrows.length) return list.replaceChildren(h('div', { class: 'empty' }, 'No escrows involve you yet.'));
    list.replaceChildren(h('div', { class: 'table-wrap' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Your role'), h('th', {}, 'Parties'), h('th', {}, 'Stage'), h('th', { style: { textAlign: 'right' } }, 'Amount'), h('th', {}, ''))),
      h('tbody', {}, escrows.map((e) => {
        const role = [e.sender === S.me.owner && 'sender', e.recipient === S.me.owner && 'recipient', e.arbiter === S.me.owner && 'arbiter'].filter(Boolean).join(', ');
        const c = e.carrier;
        const expired = c && c.mode !== 2 && S.info.tip.time >= c.expiry;
        const acts = [];
        if (c && e.stage === 'Locked' && e.arbiter === S.me.owner) {
          acts.push(h('button', { class: 'btn small', onclick: () => run({ op: 'escrow_release', commitment: e.commitment, token: c.token }, 'Released to the recipient') }, 'Release'), ' ');
          if (c.mode !== 1) acts.push(h('button', { class: 'btn ghost small', onclick: () => run({ op: 'escrow_refund', commitment: e.commitment, token: c.token }, 'Refunded to the sender') }, 'Refund'));
        }
        if (c && e.stage === 'Locked' && e.sender === S.me.owner && expired) {
          acts.push(h('button', { class: 'btn ghost small', onclick: () => run({ op: 'transfer', to: S.me.owner, tokens: [c.token], amounts: [c.zats] }, 'Taken back') }, 'Take back'));
        }
        return h('tr', {},
          h('td', {}, role),
          h('td', { class: 'mono', style: { fontSize: '12px' } }, `from ${short(e.sender)}`, h('br'), `to ${short(e.recipient)}`, h('br'), `arbiter ${short(e.arbiter)}`),
          h('td', {}, h('span', { class: `chip ${e.stage === 'Consumed' ? 'ok' : 'escrow'}` }, e.stage === 'Consumed' ? 'settled' : e.stage.toLowerCase()), c && c.mode !== 2 ? h('div', { class: 'hint', style: { margin: 0 } }, `${expired ? 'expired' : 'expires'} ${when(c.expiry)}`) : null),
          h('td', { class: 'num' }, c ? `${zec(c.zats)} ZEC` : '—'),
          h('td', { style: { textAlign: 'right', whiteSpace: 'nowrap' } }, acts));
      })))));
  }).catch((e) => list.replaceChildren(h('div', { class: 'empty' }, e.message)));

  const lock = () => {
    const tok = tokenById($('#e-tok').value);
    if (!tok) return toast('Pick a token first', false);
    run({ op: 'escrow_lock', token: tok.token, recipient: $('#e-to').value.trim(), arbiter: $('#e-arb').value.trim(), salt: randomHex(32), expiry: dateToSecs($('#e-exp').value), mode: Number($('#e-mode').value) }, 'Locked in escrow');
  };
  return [
    h('h1', {}, 'Escrow'),
    h('p', { class: 'lede' }, 'Lock a whole token for a recipient. A third party, the arbiter, releases it to them or refunds it to you. After the expiry you can take it back. Split a token first (send part of it to yourself) to escrow a smaller amount.'),
    h('div', { class: 'card' },
      h('div', { class: 'row' },
        h('label', {}, 'Token (the whole token is locked)', tokenSelect('e-tok', (t) => !t.rule)),
        h('label', {}, 'Recipient owner id', h('input', { id: 'e-to', placeholder: '0x…' })),
        h('label', {}, 'Arbiter owner id', h('input', { id: 'e-arb', placeholder: '0x…' })),
      ),
      h('div', { class: 'row', style: { marginTop: '12px' } },
        h('label', {}, 'Expires', h('input', { id: 'e-exp', type: 'datetime-local' })),
        h('label', {}, 'Kind', h('select', { id: 'e-mode' }, h('option', { value: 0 }, 'Release or refund'), h('option', { value: 1 }, 'Release only'), h('option', { value: 2 }, 'No expiry'))),
        h('button', { class: 'btn gold', onclick: lock }, 'Lock in escrow'),
      ),
    ),
    h('div', { class: 'card' }, h('h2', {}, 'Escrows you are part of'), list),
  ];
}

// ---- HTLC -----------------------------------------------------------------------------------------------------------

function htlc() {
  // kept in state: the page re-renders after every command, and this is the only copy of the secret
  const secretBox = S.share ? h('div', { class: 'card' }, h('h2', {}, 'Give this to the recipient'),
    h('p', { class: 'hint', style: { marginTop: 0 } }, 'The named recipient claims the money with it. The vault does not see the secret until they claim. Copy it now: it is not stored anywhere else.'),
    h('div', { class: 'secret' }, S.share), h('p', {}, copyBtn(S.share), ' ', h('button', { class: 'btn ghost small', onclick: () => { S.share = null; render(); } }, 'I saved it'))) : h('div');
  const lock = async () => {
    const tok = tokenById($('#hl-tok').value);
    const to = $('#hl-to').value.trim();
    if (!tok || !/^0x[0-9a-fA-F]{40}$/.test(to)) return toast('Pick a token and a recipient owner id', false);
    const secret = randomHex(32);
    const c = htlcCommitment(S.info.network_id, S.info.host, S.me.owner, to, tok.token, tok.zats, secret);
    // set before submitting, so the re-render that follows a successful lock already shows it
    S.share = JSON.stringify({ sender: S.me.owner, token: tok.token, secret });
    const ok = await run({ op: 'htlc_lock', token: tok.token, commitment: c, expiry: dateToSecs($('#hl-exp').value), mode: 0 }, 'Locked behind a secret');
    if (!ok) { S.share = null; render(); }
  };
  const claim = () => {
    let p;
    try { p = JSON.parse($('#hc-blob').value); } catch { return toast('Paste the JSON the sender gave you', false); }
    run({ op: 'htlc_claim', sender: p.sender, token: p.token, preimage: p.secret }, 'Claimed');
  };
  return [
    h('h1', {}, 'Hash lock'),
    h('p', { class: 'lede' }, 'Lock a token behind a secret. The named recipient claims it by revealing the secret; if they do not before the expiry, it comes back to you. The same pattern as atomic swaps.'),
    h('div', { class: 'grid2' },
      h('div', { class: 'card' }, h('h2', {}, 'Lock'),
        h('div', { style: { display: 'grid', gap: '12px' } },
          h('label', {}, 'Token (the whole token)', tokenSelect('hl-tok', (t) => !t.rule)),
          h('label', {}, 'Recipient owner id', h('input', { id: 'hl-to', placeholder: '0x…' })),
          h('label', {}, 'Expires', h('input', { id: 'hl-exp', type: 'datetime-local' })),
          h('button', { class: 'btn gold', onclick: lock }, 'Lock behind a new secret'))),
      h('div', { class: 'card' }, h('h2', {}, 'Claim'),
        h('div', { style: { display: 'grid', gap: '12px' } },
          h('label', {}, 'What the sender gave you', h('input', { id: 'hc-blob', placeholder: '{"sender":…,"token":…,"secret":…}' })),
          h('button', { class: 'btn', onclick: claim }, 'Claim with the secret'))),
    ),
    secretBox,
  ];
}

// ---- withdraw -------------------------------------------------------------------------------------------------------

function withdraw() {
  const list = h('div');
  api('/api/vault').then((b) => {
    const rows = Object.entries(b.payouts || {}).reverse();
    list.replaceChildren(rows.length ? h('div', { class: 'table-wrap' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, 'Log entry'), h('th', {}, 'To'), h('th', { style: { textAlign: 'right' } }, 'Amount'), h('th', {}, 'Status'))),
      h('tbody', {}, rows.map(([i, p]) => h('tr', {},
        h('td', { class: 'mono' }, i), h('td', { class: 'mono', style: { fontSize: '12px' } }, short(p.to, 8, 6)),
        h('td', { class: 'num' }, `${zec(p.zats)} ZEC`),
        h('td', {}, p.txid ? h('span', { class: 'chip ok', title: p.txid }, `paid ${short(p.txid, 4, 4)}`) : h('span', { class: 'chip time', title: p.error || '' }, 'queued'))))))) : h('div', { class: 'empty' }, 'No cheques written yet.'));
  }).catch(() => {});
  const fee = S.info.withdraw_fee_zats;
  const words = h('div', { class: 'words' });
  const micr = h('div', { class: 'micr', 'aria-hidden': 'true' });
  const upd = () => {
    const v = $('#w-amt')?.value || '', tok = tokenById($('#w-tok')?.value);
    words.replaceChildren(h('span', { class: 'k' }, 'The sum of'), v && toZats(v) ? `${inWords(v)} ZEC` : '—');
    micr.textContent = `⑆${S.me.owner.slice(2, 14)}⑆  ${tok ? serial(tok).replace(/ /g, '') : '000000000000'}⑈  ${String(toZats(v || '0')).padStart(10, '0')}⑈`;
  };
  const from = tokenSelect('w-tok');
  from.addEventListener('change', upd);
  queueMicrotask(upd);
  return [
    h('h1', {}, 'Withdraw'),
    h('p', { class: 'lede' }, 'Write the vault a cheque: it pays shielded ZEC to your Zcash address. A withdrawal is a transfer out of the VM, so every rule on the note still applies: escrowed, locked or frozen money cannot leave.'),
    h('section', { class: 'cheque' },
      h('div', { class: 'cq-head' },
        h('div', { class: 'cq-bank' }, h('span', { class: 'logo' }, 'z'), h('div', {}, h('div', { class: 'doc-title' }, 'zvm vault'), h('div', { class: 'kicker' }, 'Pays shielded ZEC on Zcash'))),
        h('div', { class: 'cq-meta' },
          h('div', {}, h('span', { class: 'k' }, 'Cheque No.'), h('span', { class: 'serial' }, String((S.hold?.nonce || 0) + 1).padStart(6, '0'))),
          h('div', {}, h('span', { class: 'k' }, 'Date'), h('span', {}, new Date().toLocaleDateString(undefined, { dateStyle: 'medium' }))))),
      h('div', { class: 'cq-pay' },
        h('label', { class: 'cq-to' }, 'Pay to the shielded address', h('input', { id: 'w-to', placeholder: 'u1… or zs1…', autocomplete: 'off' })),
        h('label', { class: 'cq-amt' }, 'ZEC', h('input', { id: 'w-amt', type: 'number', step: '0.00000001', min: '0', oninput: upd }))),
      words,
      h('div', { class: 'cq-foot' },
        h('label', {}, 'From note', from),
        h('div', { class: 'cq-sign' },
          h('div', { class: 'sig' }, short(S.me.owner, 6, 4)),
          h('div', { class: 'k' }, 'Signed with your browser key'),
          h('button', { class: 'btn gold', onclick: () => run({ op: 'withdraw', tokens: [$('#w-tok').value], amounts: [toZats($('#w-amt').value)], to: $('#w-to').value.trim() }, 'Withdrawal queued: the vault pays it on its next round') }, 'Sign and withdraw'))),
      micr),
    h('p', { class: 'hint', style: { marginBottom: '28px' } }, `The Zcash network fee of ${zec(fee)} ZEC is taken from the amount. The payment carries the memo zvm1:out:<log entry>, so anyone auditing the vault can match it to this cheque.`),
    h('div', { class: 'card' }, h('h2', {}, 'Cheques paid'), list),
  ];
}

// ---- audit ----------------------------------------------------------------------------------------------------------

function audit() {
  const i = S.info;
  const logBox = h('div', {}, h('div', { class: 'empty' }, 'Loading…'));
  const anchors = h('div');
  Promise.all([api(`/api/log?from=${Math.max(0, i.entries - 100)}&limit=100`), api('/api/vault')]).then(([log, book]) => {
    logBox.replaceChildren(log.entries.length ? h('div', { class: 'table-wrap' }, h('table', {},
      h('thead', {}, h('tr', {}, h('th', {}, '#'), h('th', {}, 'Block'), h('th', {}, 'What'), h('th', {}, 'Root after'))),
      h('tbody', {}, log.entries.slice().reverse().map((e) => {
        let what = e.kind === 'deposit' ? `deposit ${zec(e.zats)} ZEC (tx ${short(e.txid, 4, 4)})` : '';
        if (e.kind === 'command') { try { const p = JSON.parse(e.payload); what = `${p.cmd.op.replace(/_/g, ' ')} · signer ${short(ownerOf(p.signer))}`; } catch {} }
        return h('tr', {}, h('td', { class: 'mono' }, e.index), h('td', { class: 'mono' }, e.height.toLocaleString()), h('td', {}, what), h('td', { class: 'mono', title: e.root }, short(e.root, 6, 6)));
      })))) : h('div', { class: 'empty' }, 'The log is empty.'));
    anchors.replaceChildren((book.anchors || []).length ? h('ul', {}, book.anchors.slice().reverse().map((a) => h('li', { class: 'mono', style: { fontSize: '13px' } }, `entry ${a.index} · root ${short(a.root, 6, 6)} · tx ${short(a.txid, 6, 6)}`))) : h('p', { class: 'hint' }, 'No root anchored on Zcash yet.'));
  }).catch((e) => logBox.replaceChildren(h('div', { class: 'empty' }, e.message)));
  return [
    h('h1', {}, 'Audit the vault'),
    h('p', { class: 'lede' }, 'Do not trust the operator: check it. Every accepted command is in a public log with the Zcash block it ran at and the state root after it. Replaying the log from zero must give every root again.'),
    h('div', { class: 'grid2' },
      h('div', { class: 'card' }, h('h2', {}, 'Current state'),
        h('div', { class: 'field' }, h('div', { class: 'k' }, 'State root'), h('div', { class: 'v' }, h('span', {}, i.root), copyBtn(i.root))),
        h('div', { class: 'field' }, h('div', { class: 'k' }, 'Log entries'), h('div', { class: 'v' }, String(i.entries))),
        h('div', { class: 'field' }, h('div', { class: 'k' }, 'Vault viewing key fingerprint'), h('div', { class: 'v' }, i.viewing_key_fingerprint || 'stand-in chain')),
        i.viewing_key ? h('div', { class: 'field' }, h('div', { class: 'k' }, 'Vault viewing key'), h('div', { class: 'v' }, h('span', {}, short(i.viewing_key, 14, 10)), copyBtn(i.viewing_key))) : null),
      h('div', { class: 'card' }, h('h2', {}, 'Check it yourself'),
        h('ol', { class: 'steps' },
          h('li', {}, 'Download the log: ', h('a', { href: '/api/log?limit=1000', target: '_blank', rel: 'noopener' }, '/api/log')),
          h('li', {}, 'Run ', h('code', {}, 'zvm replay --dir <folder>'), ': it rebuilds the state and stops at the first entry whose root does not match.'),
          h('li', {}, 'With the viewing key, a Zcash wallet shows every deposit (memo zvm1:dep), payout (zvm1:out) and anchored root (zvm1:root) on chain.'),
        ),
        h('h2', { style: { marginTop: '16px' } }, 'Roots anchored on Zcash'), anchors),
    ),
    h('div', { class: 'card' }, h('h2', {}, 'Latest log entries'), logBox),
  ];
}

// ---- key ------------------------------------------------------------------------------------------------------------

function key() {
  const shown = h('div');
  return [
    h('h1', {}, 'Your key'),
    h('p', { class: 'lede' }, 'zvm made an ed25519 key in this browser. It signs everything you do, and your owner id comes from it. It never leaves this browser unless you copy it.'),
    h('div', { class: 'card' },
      h('div', { class: 'field' }, h('div', { class: 'k' }, 'Owner id'), h('div', { class: 'v' }, h('span', {}, S.me.owner), copyBtn(S.me.owner))),
      h('div', { class: 'field' }, h('div', { class: 'k' }, 'Public key'), h('div', { class: 'v' }, h('span', {}, S.me.pub))),
      h('div', { class: 'row' },
        h('button', { class: 'btn ghost', onclick: () => shown.replaceChildren(h('p', { class: 'hint' }, 'Anyone with this can spend your zvm money. Keep it somewhere safe.'), h('div', { class: 'secret' }, S.me.seed), h('p', {}, copyBtn(S.me.seed))) }, 'Show backup secret'),
      ), shown),
    h('div', { class: 'card' }, h('h2', {}, 'Use another key'),
      h('div', { class: 'row' },
        h('label', {}, 'Backup secret (64 hex)', h('input', { id: 'k-in', autocomplete: 'off' })),
        h('button', { class: 'btn', onclick: async () => {
          const v = $('#k-in').value.trim().replace(/^0x/, '');
          if (!/^[0-9a-fA-F]{64}$/.test(v)) return toast('A backup secret is 64 hex characters', false);
          try { localStorage.setItem('zvm.seed', v); } catch {}
          await loadKey(); await refresh(); toast('Switched key');
        } }, 'Switch'),
        h('button', { class: 'btn ghost', onclick: async () => {
          try { localStorage.setItem('zvm.seed', randomHex(32)); } catch {}
          await loadKey(); await refresh(); toast('New key: back it up before you deposit');
        } }, 'New key'),
      )),
  ];
}

// ---- boot -----------------------------------------------------------------------------------------------------------

(async () => {
  if (!crypto?.subtle) {
    $('#main').replaceChildren(h('div', { class: 'card' }, 'zvm needs a secure page (https or localhost) to make your key.'));
    return;
  }
  try {
    await loadKey();
  } catch {
    $('#main').replaceChildren(h('div', { class: 'card' }, 'This browser cannot make ed25519 keys (WebCrypto). Use a current Chrome, Firefox or Safari.'));
    return;
  }
  const v = location.hash.slice(1);
  if (SECTIONS.some((s) => s && s[0] === v)) S.view = v;
  await refresh();
  // keep the clock, balance and deposits fresh without touching what the user is typing in
  setInterval(async () => {
    try {
      const [info, hold] = await Promise.all([api('/api/info'), api(`/api/owner/${S.me.owner}`)]);
      const changed = JSON.stringify(hold) !== JSON.stringify(S.hold);
      S.info = info; S.hold = hold;
      renderTop();
      if (changed && (S.view === 'wallet' || S.view === 'deposit')) render();
      if (changed && S.view === 'deposit') toast('Deposit credited');
    } catch {}
  }, 10000);
})();
