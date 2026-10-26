// End-to-end walkthrough of the web app on a stand-in chain, with three users in three browsers.
//   zvm serve --dir <tmp> --fake --port 8792 --web web   then   node e2e/walk.mjs <screenshot dir>
// Playwright is resolved from PLAYWRIGHT_FROM (any folder whose node_modules has playwright).
import { createRequire } from "node:module";
const require = createRequire((process.env.PLAYWRIGHT_FROM || process.cwd()) + "/package.json");
const { chromium } = require("playwright");
const out = process.argv[2] || ".", base = process.env.ZVM_URL || "http://127.0.0.1:8792";
const browser = await chromium.launch();
const errors = [];
const mk = async (opts = {}) => { const c = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts }); const p = await c.newPage(); p.on("pageerror", e => errors.push(e.message)); return p; };
const alice = await mk(), bob = await mk(), carol = await mk();
for (const p of [alice, bob, carol]) { await p.goto(base + "/#wallet"); await p.waitForSelector(".idline code"); }
const id = p => p.evaluate(() => S.me.owner);
const A = await id(alice), B = await id(bob), C = await id(carol);
const toast = async p => { const t = await p.waitForSelector(".toast", { timeout: 8000 }); const s = await t.textContent(); await p.evaluate(() => document.querySelector(".toast")?.remove()); return s; };
const nav = (p, name) => p.click(`nav >> text=${name}`);
const send = async (p, amt, to) => { await nav(p, "Send"); await p.fill("#s-amt", amt); await p.fill("#s-to", to); await p.click("main >> button:has-text('Send')"); return toast(p); };
const pickToken = async (p, sel, amt) => { const o = await p.$$eval(`${sel} option`, os => os.map(o => [o.value, o.textContent])); await p.selectOption(sel, o.find(x => x[1].endsWith(` ${amt} ZEC`))[0]); };
const results = [];
const check = (name, got, want) => { const ok = got.includes(want); results.push([ok, name, got]); console.log(ok ? "✓" : "✗", name, "→", got); };

await nav(alice, "Deposit"); await alice.fill("#dep-amt", "0.5"); await alice.click("text=Simulate a");
check("deposit", await toast(alice), "credited");
check("send 0.1 to bob", await send(alice, "0.1", B), "Sent");
await nav(alice, "Program"); await alice.click(".tpl:has-text('Spending cap')"); await alice.fill("#p-cap", "0.05");
await alice.screenshot({ path: out + "/2-program.png", fullPage: true });
await alice.click("text=Apply rule");
check("issuer cap set", await toast(alice), "Rules set");
check("alice over cap refused", await send(alice, "0.08", C), "refused");
await bob.reload(); await bob.waitForSelector(".idline code");
check("bob over cap refused (rule followed the money)", await send(bob, "0.06", C), "refused");
check("split 0.04 to self", await send(alice, "0.04", A), "Sent");
await nav(alice, "Escrow"); await pickToken(alice, "#e-tok", "0.04");
await alice.fill("#e-to", B); await alice.fill("#e-arb", C); await alice.fill("#e-exp", "2030-01-01T00:00");
await alice.click("text=Lock in escrow");
check("escrow lock", await toast(alice), "escrow");
await carol.reload(); await carol.waitForSelector(".idline code"); await nav(carol, "Escrow"); await carol.waitForSelector("button:has-text('Release')");
await carol.screenshot({ path: out + "/3-escrow.png", fullPage: true });
await carol.click("button:has-text('Release')");
check("arbiter release", await toast(carol), "Released");
check("split 0.03 to self", await send(alice, "0.03", A), "Sent");
await nav(alice, "Hash lock"); await pickToken(alice, "#hl-tok", "0.03");
await alice.fill("#hl-to", B); await alice.fill("#hl-exp", "2030-01-01T00:00");
await alice.click("text=Lock behind a new secret");
check("hash lock", await toast(alice), "Locked");
const blob = (await alice.textContent(".secret")).trim();
await nav(bob, "Hash lock"); await bob.fill("#hc-blob", blob); await bob.click("text=Claim with the secret");
check("claim with secret", await toast(bob), "Claimed");
await nav(bob, "Withdraw"); await bob.fill("#w-amt", "0.02"); await bob.fill("#w-to", "u1example"); await bob.click("main >> button:has-text('Withdraw')");
check("withdraw", await toast(bob), "queued");

await nav(bob, "Wallet"); await bob.waitForSelector(".tok");
await bob.screenshot({ path: out + "/4-bob-wallet.png", fullPage: true });
await nav(alice, "Wallet"); await alice.waitForSelector(".tok");
await alice.screenshot({ path: out + "/1-alice-wallet.png", fullPage: true });
await nav(alice, "Audit"); await alice.waitForSelector("tbody tr");
await alice.screenshot({ path: out + "/5-audit.png", fullPage: true });
const phone = await mk({ viewport: { width: 390, height: 844 } });
await phone.goto(base + "/#wallet"); await phone.waitForSelector(".idline code");
await phone.screenshot({ path: out + "/6-phone.png", fullPage: true });
check("no sideways scroll on a phone", String(!(await phone.evaluate(() => document.documentElement.scrollWidth > window.innerWidth))), "true");
console.log("page errors:", errors.length ? errors : "none");
await browser.close();
process.exit(results.every(r => r[0]) && !errors.length ? 0 : 1);
