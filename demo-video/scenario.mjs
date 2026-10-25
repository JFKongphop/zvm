// zvm demo video: the real web app on a stand-in chain (`zvm serve --fake`), recorded by the
// hyperframes-app-demo skill. Run with .claude/skills/zvm-demo-video/scripts/make-demo.sh from zvm/.
//
// Filmed as Alice in one browser. Carol (the arbiter) and Bob (a recipient) are fixed demo keys; the same
// window switches to them on camera through the key page, the way a user would.

const BOB = 'b0b0'.repeat(16);
const CAROL = 'ca7e'.repeat(16);

export const config = {};

// the app's banknote look: paper and ink, Fraunces figures, red serials, double-rule frames
const PAPER = '#f3eee2', PAPER2 = '#e8e1cf', INK = '#1f2b26', INK2 = '#4c5a52', INK3 = '#7e877f', RULE = 'rgba(31,43,38,.38)', RED = '#a3322b', GREEN = '#2f6b4a', SEAL = '#f4b728';
const frame = `border: 1px solid ${INK}; border-radius: 1px; box-shadow: inset 0 0 0 5px ${PAPER}, inset 0 0 0 6px ${RULE};`;
const smallcaps = `font-family: 'IBM Plex Sans'; font-weight: 600; letter-spacing: .18em; text-transform: uppercase;`;

export const video = {
  brand: { name: 'zvm', mark: 'z', accent: SEAL, accent2: GREEN, alert: RED, ink: INK, bg: PAPER, fontDisplay: 'Fraunces', fontMono: 'IBM Plex Mono' },
  fontLinks: ['https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,300;9..144,400&family=IBM+Plex+Sans:wght@500;600&display=swap'],
  intro: {
    title: 'Shielded ZEC that follows rules.',
    highlight: 'Programmable money.',
    chips: ['Shielded in & out', '256-bit moneyvm rules', 'Replayable log'],
    seconds: 6.5,
  },
  problem: {
    eyebrow: 'THE PROBLEM',
    stat: '0',
    note: 'general-purpose smart contracts on Zcash today',
    lines: [
      'The most private money has <b>no programmable rules</b>: no escrow, no expiry, no spending caps.',
      'zvm attaches a <b>moneyvm instruction</b> to the money and checks it on every move.',
    ],
    seconds: 8,
  },
  explainer: [
    {
      kind: 'redacted', eyebrow: 'HOW IT WORKS', title: 'Money enters and leaves as shielded ZEC',
      rows: ['Sender', 'Recipient', 'Amount', 'Memo'],
      note: 'A deposit is a shielded payment to the vault. Its memo names your owner id, and the vault <b>mints you a token</b>.',
      noteAt: 3, seconds: 7,
    },
    {
      kind: 'text', eyebrow: 'HOW IT WORKS', title: 'Every transfer runs the rules. Every step can be checked.',
      note: 'Commands are signed in your browser and written to a public log. <b>Anyone can replay it</b> and get the same state, and its root is anchored in a Zcash memo.',
      seconds: 8,
    },
  ],
  footage: { startAt: /^<b>1 ·/, pill: '● REAL APP · STAND-IN CHAIN' },
  chapters: [
    { match: /^<b>1 ·/, label: 'Deposit shielded ZEC' },
    { match: /^<b>2 ·/, label: 'Money as notes' },
    { match: /^<b>3 ·/, label: 'Send to Bob' },
    { match: /^<b>4 ·/, label: 'Program a cap' },
    { match: /^<b>5 ·/, label: 'The rule refuses' },
    { match: /^<b>6 ·/, label: 'Escrow with an arbiter' },
    { match: /^<b>7 ·/, label: 'Arbiter releases' },
    { match: /^<b>8 ·/, label: 'Hash lock + claim' },
    { match: /^<b>9 ·/, label: 'Cheque out, shielded' },
    { match: /^<b>10 ·/, label: 'Audit the log' },
  ],
  railTitle: 'Programmable ZEC',
  outro: {
    items: [{ k: 'ENGINE', v: 'moneyvm in Rust' }, { k: 'MONEY', v: 'Shielded ZEC' }, { k: 'TRUST', v: 'Replayable log' }],
    tagline: 'zvm · programmable shielded ZEC',
    seconds: 8,
  },
  css: `
      #root { background-color: ${PAPER}; background-image: none; }
      .box { background: ${PAPER}; ${frame} }
      .logo .c { background: ${SEAL}; color: ${INK}; border: 1px solid ${INK}; border-radius: 50%; box-shadow: 0 0 0 3px ${PAPER}, 0 0 0 4px ${INK}; font-weight: 600; }
      #topbar .logo .c { box-shadow: 0 0 0 2px ${PAPER}, 0 0 0 3px ${INK}; }
      .logo .w { font-weight: 500; }
      #intro h1 { font-weight: 400; letter-spacing: -.025em; }
      #t2 span { background: ${INK}; color: ${PAPER}; border: 0; border-radius: 0; box-shadow: none; font-weight: 400; font-style: italic; }
      .chip { background: transparent; color: ${INK}; border: 1px solid ${INK}; border-radius: 0; box-shadow: none; ${smallcaps} font-size: 17px; }
      .chip.live { color: ${GREEN}; border-color: ${GREEN}; }
      #p-eyebrow { color: ${RED}; ${smallcaps} font-size: 20px; }
      #p-num { color: ${RED}; font-weight: 300; }
      #p-sub { color: ${INK2}; }
      .pl { font-weight: 400; }
      .pl b, .ex-note b { background: none; border: 0; padding: 0; font-style: italic; color: ${RED}; }
      .ex-eyebrow, #rail h3 { color: ${INK2}; ${smallcaps} font-size: 18px; }
      .ex-title, #rail h2 { font-weight: 400; }
      .ex-note { font-weight: 400; }
      .ex-red { background: ${PAPER}; ${frame} }
      .ex-row { border-bottom: 1px dotted ${RULE}; }
      .ex-row i { background: repeating-linear-gradient(90deg, ${INK} 0 1px, transparent 1px 3px); border-radius: 0; height: 22px; }
      .pill { background: transparent; color: ${GREEN}; border: 1px solid ${GREEN}; border-radius: 0; box-shadow: none; ${smallcaps} font-size: 15px; }
      #frame { border-radius: 1px; } #vid { border-radius: 0; }
      #barwrap { border: 1px solid ${INK}; background: ${PAPER}; border-radius: 0; }
      #bar { background: ${INK}; }
      .cap { color: ${INK}; font-family: 'IBM Plex Sans'; font-weight: 500; } .cap b { background: ${INK}; color: ${PAPER}; border: 0; border-radius: 0; }
      #hl { background: ${PAPER2}; border: 1px solid ${INK}; border-radius: 0; box-shadow: inset 0 0 0 3px ${PAPER2}, inset 0 0 0 4px ${RULE}; }
      .row .n { color: ${RED}; } .row .lbl { font-weight: 400; } .row .tick { color: ${GREEN}; }
      .stack { border-radius: 0; ${frame} }
      .stack .k { color: ${INK3}; ${smallcaps} font-size: 15px; }
      .stack .v { font-weight: 400; }
      .stack.s0 { background: ${PAPER}; } .stack.s1 { background: ${INK}; color: ${PAPER}; box-shadow: none; } .stack.s1 .k { color: rgba(243,238,226,.65); }
      .stack.s2 { background: ${PAPER}; color: ${GREEN}; } .stack.s2 .k { color: ${GREEN}; }
      #o-live { color: ${INK2}; }
`,
};

export default async function scenario({ page, goto, caption, click, type, point, scrollTo, scrollY, hold, waitFor }) {
  const nav = (name) => click(page.locator('nav#nav').getByRole('button', { name, exact: true }));
  const main = page.locator('#main');
  // long hex ids are pasted, not typed: typing 42 characters would take six seconds of nothing
  const paste = async (sel, text) => { const l = page.locator(sel); await point(l); await l.fill(text); await hold(500); };
  const toast = (label, ok) => waitFor(label, page.locator('.toast', { hasText: ok }), { error: page.locator('.toast.bad'), timeout: 20000 });
  const pick = async (sel, zecAmount) => {
    const opts = await page.$$eval(`${sel} option`, (os) => os.map((o) => [o.value, o.textContent]));
    const hit = opts.find((o) => o[1].endsWith(` ${zecAmount} ZEC`));
    if (!hit) throw new Error(`no ${zecAmount} ZEC token in ${sel}: ${opts.map((o) => o[1]).join(' | ')}`);
    await point(page.locator(sel)); await page.selectOption(sel, hit[0]); await hold(600);
  };
  const switchTo = async (seedHex, name) => {
    await click(page.locator('button.me'));
    await page.locator('#k-in').waitFor();
    await paste('#k-in', seedHex);
    await click(main.getByRole('button', { name: 'Switch', exact: true }));
    await toast(`switch to ${name}`, 'Switched key');
    await hold(1200);
  };

  await goto('/#wallet');
  await page.locator('.idline code').waitFor();
  const ALICE_SEED = await page.evaluate(() => S.me.seed);
  const [B, C] = await page.evaluate(async ([b, c]) => [(await keyFromSeed(b)).owner, (await keyFromSeed(c)).owner], [BOB, CAROL]);
  const A = await page.evaluate(() => S.me.owner);
  await hold(1200);

  // 1 · deposit
  await caption('<b>1 ·</b> Deposit shielded ZEC: the memo names your owner id');
  await nav('Deposit');
  await type(page.locator('#dep-amt'), '0.5');
  await point(main.locator('.slip-stub'));
  await hold(2500);
  await click(main.getByRole('button', { name: /Simulate a 0\.5 ZEC deposit/ }));
  await toast('deposit credited', 'Deposit credited');
  await page.locator('.tok').first().waitFor();
  // two more small deposits off camera, so later steps have whole tokens of the right size to lock
  await page.evaluate(async (owner) => {
    for (const zats of [5000000, 3000000]) await api('/api/dev/deposit', { owner, zats, bound: 0, time_mode: 0, payload: 0 });
    await refresh(); go('wallet');
  }, A);
  await hold(1500);

  // 2 · wallet
  await caption('<b>2 ·</b> Each deposit becomes a note you hold, its rules printed on it');
  await point(page.locator('.tok').first());
  await hold(3500);

  // 3 · send
  await caption('<b>3 ·</b> Send 0.1 ZEC to Bob, signed in the browser');
  await nav('Send');
  await pick('#s-tok', '0.5');
  await type(page.locator('#s-amt'), '0.1');
  await paste('#s-to', B);
  await click(main.getByRole('button', { name: 'Send', exact: true }));
  await toast('send', 'Sent');
  await hold(2500);

  // 4 · program
  await caption('<b>4 ·</b> Program the money: a 0.05 ZEC spending cap, as a 256-bit instruction');
  await nav('Program');
  await click(main.locator('.tpl', { hasText: 'Spending cap' }));
  await pick('#p-tok', '0.4');
  await type(page.locator('#p-cap'), '0.05');
  await scrollTo(main.locator('.word').first(), 160);
  await hold(3500);
  await scrollY(0, 900);
  await click(main.getByRole('button', { name: 'Apply rule' }));
  await toast('rule set', 'Rules set');
  await hold(2500);

  // 5 · refused
  await caption('<b>5 ·</b> Try to send 0.08 ZEC: the rule refuses, and nothing moves');
  await nav('Send');
  await pick('#s-tok', '0.4');
  await type(page.locator('#s-amt'), '0.08');
  await paste('#s-to', C);
  await click(main.getByRole('button', { name: 'Send', exact: true }));
  await page.locator('.toast.bad').waitFor({ timeout: 20000 });
  await hold(3500);

  // 6 · escrow
  await caption('<b>6 ·</b> Lock 0.05 ZEC in escrow for Bob, with Carol as arbiter');
  await nav('Escrow');
  await pick('#e-tok', '0.05');
  await paste('#e-to', B);
  await paste('#e-arb', C);
  await page.locator('#e-exp').fill('2030-01-01T00:00');
  await point(page.locator('#e-exp')); await hold(600);
  await click(main.getByRole('button', { name: 'Lock in escrow' }));
  await toast('escrow lock', 'Locked in escrow');
  await scrollTo(main.locator('table'), 200);
  await hold(2500);

  // 7 · arbiter release
  await caption('<b>7 ·</b> Carol, the arbiter, opens zvm and releases it to Bob');
  await switchTo(CAROL, 'Carol');
  await nav('Escrow');
  const release = main.getByRole('button', { name: 'Release', exact: true });
  await waitFor('escrow list', release);
  await click(release);
  await toast('release', 'Released');
  await hold(2500);

  // 8 · hash lock, then Bob claims
  await caption('<b>8 ·</b> Hash lock: Alice locks 0.03 ZEC behind a secret, Bob claims it');
  await switchTo(ALICE_SEED, 'Alice');
  await nav('Hash lock');
  await pick('#hl-tok', '0.03');
  await paste('#hl-to', B);
  await page.locator('#hl-exp').fill('2030-01-01T00:00');
  await click(main.getByRole('button', { name: 'Lock behind a new secret' }));
  await toast('hash lock', 'Locked behind a secret');
  await point(main.locator('.secret'));
  await hold(2500);
  const secret = (await main.locator('.secret').textContent()).trim();
  await switchTo(BOB, 'Bob');
  await nav('Hash lock');
  await paste('#hc-blob', secret);
  await click(main.getByRole('button', { name: 'Claim with the secret' }));
  await toast('claim', 'Claimed');
  await hold(1500);
  await nav('Wallet');
  await page.locator('.tok').first().waitFor();
  await point(page.locator('.tok').first());
  await hold(3000);

  // 9 · withdraw
  await caption('<b>9 ·</b> Bob writes the vault a cheque: shielded ZEC out, every rule checked first');
  await nav('Withdraw');
  await pick('#w-tok', '0.1');
  await type(page.locator('#w-amt'), '0.02');
  await paste('#w-to', 'u1example');
  await click(main.getByRole('button', { name: 'Sign and withdraw' }));
  await toast('withdraw', 'Withdrawal queued');
  await hold(2500);

  // 10 · audit
  await caption('<b>10 ·</b> Anyone can download the signed log, replay it and check every root');
  await nav('Audit');
  await page.locator('tbody tr').first().waitFor();
  await hold(1500);
  await scrollTo(main.locator('tbody tr').first(), 220);
  await hold(4000);
  await scrollY(0, 1200);
  await hold(1500);
}
