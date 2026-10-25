// zvm "story" cut: narrated, full-screen app scenes between bold type cards (the style of a modern product film),
// as an alternative to the chaptered cut in scenario.mjs. Recorded at 16:9 so the app fills the frame.
// Run: .claude/skills/zvm-demo-video/scripts/make-story.sh (from zvm/). Output: demo-video/zvm-story.mp4
//
// Each scene marker is a caption "<b>sN ·</b>"; compose-story.mjs cuts the footage between markers and fits it to
// the narration line of that scene.

const BOB = 'b0b0'.repeat(16);
const CAROL = 'ca7e'.repeat(16);

export const config = {};

export const story = {
  // a third voice, distinct from the Airbag and Contingentine films
  voice: { id: 'nPczCjzI2devNBz1zQrb', model: 'eleven_multilingual_v2', settings: { stability: 0.5, similarity_boost: 0.8, style: 0.2, use_speaker_boost: true } },
  envFile: '../../airbag-insure/.env',
  url: 'zvm-demo.fly.dev',
  scenes: [
    { card: ['shielded ZEC is the most', '<em>private</em> money there is.'],
      vo: 'Shielded Zcash is the most private money there is.' },
    { card: ['but it can’t follow', 'a single <em>rule</em>.'], sub: 'no escrow. no expiry. no spending limit.',
      vo: 'But it can’t follow a single rule. No escrow. No expiry. No spending limit.' },
    { card: ['zvm makes shielded ZEC', '<em>programmable</em>.'], sub: 'a 256-bit moneyvm instruction rides with every note.',
      vo: 'zvm makes it programmable. Every note carries a small program that runs on every move, and the money still comes in, and goes out, shielded.' },
    { clip: 's1', vo: 'It starts with a deposit slip. Send shielded ZEC with this memo, and the vault issues you a note.' },
    { clip: 's2', vo: 'Each note prints its rules on its face, with its own serial and engraving.' },
    { card: ['money that knows', 'its own <em>rules</em>.'], sub: 'checked on every single transfer.', hold: 2.6 },
    { clip: 's3', vo: 'Send a tenth of a ZEC to Bob. Your browser signs it. The vault checks it.' },
    { clip: 's4', vo: 'Now program the money: no single transfer above five hundredths of a ZEC. One two-hundred-fifty-six-bit instruction. Set once, for good.' },
    { clip: 's5', vo: 'Now try to send more than the cap. The rule refuses, and nothing moves. Not a single zatoshi.' },
    { card: ['the rule follows', 'the <em>money</em>.'], sub: 'even after it changes hands.', hold: 2.6 },
    { clip: 's6', vo: 'Escrow. Alice locks a note for Bob, with Carol as the arbiter. Only Carol can release it, or send it back.' },
    { clip: 's7', vo: 'Carol opens zvm with her own key, and releases it to Bob.' },
    { clip: 's8', vo: 'A hash lock works like an atomic swap. Alice locks the money behind a secret and hands Bob the secret. Bob pastes it in, and the money is his.' },
    { clip: 's9', vo: 'To leave, Bob writes the vault a cheque. Every rule is checked one last time, then the vault pays out in shielded ZEC.' },
    { card: ['don’t trust the operator.', '<em>replay</em> it.'], sub: 'every command signed. every root anchored on Zcash.', hold: 2.6 },
    { clip: 's10', vo: 'Every command is signed and logged. Anyone can replay the log, check every state root, and match it to the roots anchored on Zcash.' },
    { card: ['programmable money.', 'still <em>private</em>.'], sub: 'zvm-demo.fly.dev', end: true,
      vo: 'zvm. Programmable money that stays private. Try it now, at zvm demo dot fly dot dev.' },
  ],
};

export default async function scenario({ page, goto, caption, click, type, point, scrollTo, scrollY, hold, waitFor }) {
  const nav = (name) => click(page.locator('nav#nav').getByRole('button', { name, exact: true }));
  const main = page.locator('#main');
  const paste = async (sel, text) => { const l = page.locator(sel); await point(l); await l.fill(text); await hold(400); };
  const toast = (label, ok) => waitFor(label, page.locator('.toast', { hasText: ok }), { error: page.locator('.toast.bad'), timeout: 20000 });
  const pick = async (sel, zecAmount) => {
    const opts = await page.$$eval(`${sel} option`, (os) => os.map((o) => [o.value, o.textContent]));
    const hit = opts.find((o) => o[1].endsWith(` ${zecAmount} ZEC`));
    if (!hit) throw new Error(`no ${zecAmount} ZEC token in ${sel}: ${opts.map((o) => o[1]).join(' | ')}`);
    await point(page.locator(sel)); await page.selectOption(sel, hit[0]); await hold(400);
  };
  const switchTo = async (seedHex, name) => {
    await click(page.locator('button.me'));
    await page.locator('#k-in').waitFor();
    await paste('#k-in', seedHex);
    await click(main.getByRole('button', { name: 'Switch', exact: true }));
    await toast(`switch to ${name}`, 'Switched key');
    await hold(600);
  };

  await goto('/#wallet');
  await page.locator('.idline code').waitFor();
  const ALICE_SEED = await page.evaluate(() => S.me.seed);
  const [B, C] = await page.evaluate(async ([b, c]) => [(await keyFromSeed(b)).owner, (await keyFromSeed(c)).owner], [BOB, CAROL]);
  const A = await page.evaluate(() => S.me.owner);
  await hold(800);

  await caption('<b>s1 ·</b> deposit slip');
  await nav('Deposit');
  await type(page.locator('#dep-amt'), '0.5');
  await point(main.locator('.words'));
  await hold(1500);
  await click(main.getByRole('button', { name: /Simulate a 0\.5 ZEC deposit/ }));
  await toast('deposit credited', 'Deposit credited');
  await page.evaluate(async (owner) => {
    for (const zats of [5000000, 3000000]) await api('/api/dev/deposit', { owner, zats, bound: 0, time_mode: 0, payload: 0 });
    await refresh(); go('wallet');
  }, A);
  await page.locator('.tok').first().waitFor();

  await caption('<b>s2 ·</b> notes');
  await hold(800);
  await scrollTo(page.locator('.tok').first(), 140, 1200);
  await point(page.locator('.tok .terms').first());
  await hold(2500);
  await scrollY(0, 800);

  await caption('<b>s3 ·</b> send');
  await nav('Send');
  await pick('#s-tok', '0.5');
  await type(page.locator('#s-amt'), '0.1');
  await paste('#s-to', B);
  await click(main.getByRole('button', { name: 'Send', exact: true }));
  await toast('send', 'Sent');
  await hold(1500);

  await caption('<b>s4 ·</b> program');
  await nav('Program');
  await click(main.locator('.tpl', { hasText: 'Spending cap' }));
  await pick('#p-tok', '0.4');
  await type(page.locator('#p-cap'), '0.05');
  await scrollTo(main.locator('.word').first(), 220, 900);
  await hold(2500);
  await click(main.getByRole('button', { name: 'Apply rule' }));
  await toast('rule set', 'Rules set');
  await hold(1200);
  await scrollY(0, 700);

  await caption('<b>s5 ·</b> refused');
  await nav('Send');
  await pick('#s-tok', '0.4');
  await type(page.locator('#s-amt'), '0.08');
  await paste('#s-to', C);
  await click(main.getByRole('button', { name: 'Send', exact: true }));
  await page.locator('.toast.bad').waitFor({ timeout: 20000 });
  await hold(2600);

  await caption('<b>s6 ·</b> escrow');
  await nav('Escrow');
  await pick('#e-tok', '0.05');
  await paste('#e-to', B);
  await paste('#e-arb', C);
  await page.locator('#e-exp').fill('2030-01-01T00:00');
  await click(main.getByRole('button', { name: 'Lock in escrow' }));
  await toast('escrow lock', 'Locked in escrow');
  await scrollTo(main.locator('table'), 260, 900);
  await hold(1500);

  await caption('<b>s7 ·</b> release');
  await switchTo(CAROL, 'Carol');
  await nav('Escrow');
  const release = main.getByRole('button', { name: 'Release', exact: true });
  await waitFor('escrow list', release);
  await click(release);
  await toast('release', 'Released');
  await hold(1600);

  await caption('<b>s8 ·</b> hash lock');
  await switchTo(ALICE_SEED, 'Alice');
  await nav('Hash lock');
  await pick('#hl-tok', '0.03');
  await paste('#hl-to', B);
  await page.locator('#hl-exp').fill('2030-01-01T00:00');
  await click(main.getByRole('button', { name: 'Lock behind a new secret' }));
  await toast('hash lock', 'Locked behind a secret');
  await point(main.locator('.secret'));
  await hold(1400);
  const secret = (await main.locator('.secret').textContent()).trim();
  await switchTo(BOB, 'Bob');
  await nav('Hash lock');
  await paste('#hc-blob', secret);
  await click(main.getByRole('button', { name: 'Claim with the secret' }));
  await toast('claim', 'Claimed');
  await hold(1400);

  await caption('<b>s9 ·</b> cheque');
  await nav('Withdraw');
  await paste('#w-to', 'u1example');
  await type(page.locator('#w-amt'), '0.02');
  await pick('#w-tok', '0.1');
  await point(main.locator('.cq-sign .sig'));
  await hold(1200);
  await click(main.getByRole('button', { name: 'Sign and withdraw' }));
  await toast('withdraw', 'Withdrawal queued');
  await hold(1600);

  await caption('<b>s10 ·</b> audit');
  await nav('Audit');
  await page.locator('tbody tr').first().waitFor();
  await hold(1200);
  await scrollTo(main.locator('tbody tr').first(), 260, 1200);
  await hold(3000);
  await caption('<b>end ·</b>');
  await hold(500);
}
