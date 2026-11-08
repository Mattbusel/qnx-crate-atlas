// Records the demo video: headless Chrome over the real site, the codelab and the upstream PR, plus
// terminal scenes that run the real commands while recording: atlas doctor, atlas run and atlas probe on
// this Windows host, and a QNX 8.0 VM booting under QEMU/KVM in WSL2 and running the examples over ssh.
// Each process's output is streamed into an xterm.js terminal as it arrives (nothing is replayed).
// Narrated with Piper (local TTS, no music), assembled with ffmpeg.   node scripts/demo/record.mjs
import fs from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import puppeteer from "puppeteer-core";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SITE = process.env.SITE || "https://qnx-crate-atlas.vercel.app";
const CHROME = process.env.CHROME_PATH || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const VOICE = process.env.PIPER_MODEL || "C:/Users/Matthew/hackathons/tts/en_US-ryan-high.onnx";
const ATLAS = process.env.ATLAS || path.join(ROOT, "work", "atlas.exe");
const WSL_DISTRO = process.env.WSL_DISTRO || "Ubuntu-24.04";
// x86_64 builds of examples/socket2-keepalive, examples/tokio-net and examples/rustls-aws-lc, as WSL paths
const BIN = process.env.QNX_BIN_DIR || "/mnt/c/Users/Matthew/.cargo/cargo-target/x86_64-pc-nto-qnx800/debug";
const EXAMPLES = ["socket2-keepalive", "tokio-net", "rustls-aws-lc"];
const OUT = path.join(ROOT, "demo-out", new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19));
const FR = path.join(OUT, "frames");
fs.mkdirSync(FR, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const N = JSON.parse(fs.readFileSync(path.join(ROOT, "demo-out", "cap", "numbers.json"), "utf8"));
const fmt = (x) => x.toLocaleString("en-US");

const SCENES = [
  { k: "open", say: `Q.N.X. ships a Rust compiler for Q.N.X. eight. The question every porting team asks is: will my dependencies build? Q.N.X. Crate Atlas answers it for the ${N.crates} most downloaded crates on crates dot I.O., on both Q.N.X. eight targets.` },
  { k: "doctor", say: "This is the Atlas running live on a Windows host. Doctor finds the S.D.P., the Q.N.X. rust compiler, Q.C.C. and the license, and shows what the compiler reports for each target: operating system N.T.O., environment N.T.O. eighty." },
  { k: "run", say: "For every crate it builds a throwaway Q.N.X. binary that links the crate with Q.C.C., on x86 and arm. When a build fails, it records the first error and sorts it into a class. If lib C is the cause, it retries with lib C pinned. Probe checks any single crate, here for arm." },
  { k: "tiles", say: `Results. Out of the box, ${N.x86_oob} percent build for x86, ${N.arm_oob} for arm. With lib C pinned, ${N.x86_pin} percent. With our three patches, ${N.x86_compat} percent. A pass means the crate compiles and links. On arm, four crates built on rustls still fail to link once rustls is really used, and the page says so.` },
  { k: "breaks", say: "The biggest cause is not Q.N.X. code at all. Upstream Rust renamed the Q.N.X. eight targets, new lib C follows the new name, and the compiler Q.N.X. ships still uses the old one. The table shows the few crates that block the most others." },
  { k: "fixes", say: "So we fixed them. A lib C compatibility patch maps the old name onto the new one, which brings socket two, and with it Tokio networking and request, back. And a fix for A.W.S. L.C., the default rustls crypto, is now an upstream pull request." },
  { k: "ontarget", say: `Now the device. This is a Q.N.X. eight kernel booting live, under Q.E.M.U. with K.V.M., inside W.S.L. on Windows. Over S.S.H. it runs three programs built by the Q.N.X. toolchain: a socket keepalive test, Tokio with every feature on, and a rustls binary. The same V.M. ran the unit tests of ${N.ot_ran} top crates: ${N.ot_clean} passed every test, ${fmt(N.ot_passed)} tests in all.` },
  { k: "codelab", say: "And a codelab, Rust on Q.N.X. eight from a Windows host, takes a fresh P.C. to a binary running on Q.N.X., including every trap we hit. A script runs its build commands." },
  { k: "outro", say: "Q.N.X. Crate Atlas. Search it before you add a dependency. Open source, M.I.T. licensed." },
];

function voice() {
  const dur = {};
  for (const s of SCENES) {
    const f = path.join(OUT, `${s.k}.wav`);
    const r = spawnSync("python", ["-m", "piper", "-m", VOICE, "-f", f, "--length-scale", "0.97", "--sentence-silence", "0.25"], { input: s.say, encoding: "utf8" });
    if (r.status !== 0) throw new Error("piper: " + r.stderr);
    dur[s.k] = Number(spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f], { encoding: "utf8" }).stdout.trim());
  }
  return dur;
}

const CAP_CSS = `#cap{position:fixed;left:50%;bottom:10px;transform:translateX(-50%);width:min(1240px,94vw);background:rgba(8,10,14,.9);color:#f2f4f6;
  font:500 18px/1.35 system-ui,sans-serif;padding:9px 16px;border-radius:10px;z-index:99999;text-align:center;pointer-events:none;border:1px solid rgba(255,255,255,.08)}`;
// captions read the narration without the spelled-out acronyms
const plain = (t) => t.replace(/Q\.N\.X\./g, "QNX").replace(/S\.D\.P\./g, "SDP").replace(/Q\.C\.C\./g, "qcc").replace(/N\.T\.O\./g, "nto").replace(/lib C/g, "libc")
  .replace(/crates dot I\.O\./g, "crates.io").replace(/Q\.E\.M\.U\./g, "QEMU").replace(/K\.V\.M\./g, "KVM").replace(/W\.S\.L\./g, "WSL").replace(/A\.W\.S\. L\.C\./g, "aws-lc-rs")
  .replace(/S\.S\.H\./g, "SSH").replace(/V\.M\./g, "VM")
  .replace(/P\.C\./g, "PC").replace(/M\.I\.T\./g, "MIT").replace(/socket two/g, "socket2").replace(/request,/g, "reqwest,").replace(/Q\.N\.X\. eight/g, "QNX 8.0").replace(/QNX eight/g, "QNX 8.0").replace(/environment nto eighty/g, "environment nto80");

async function main() {
  const dur = voice();
  const total = Object.values(dur).reduce((a, b) => a + b, 0);
  console.log("narration seconds", total.toFixed(1));
  const browser = await puppeteer.launch({ executablePath: CHROME, headless: "new", defaultViewport: { width: 1536, height: 864, deviceScaleFactor: 1.25 }, args: ["--no-first-run", "--lang=en-US", "--force-color-profile=srgb"] });
  const page = await browser.newPage();
  const prep = () => page.evaluate((css) => { if (!document.getElementById("cap")) { const st = document.createElement("style"); st.textContent = css; document.head.append(st); const c = document.createElement("div"); c.id = "cap"; document.body.append(c); } }, CAP_CSS);
  const caption = (t) => page.evaluate((t) => { const c = document.getElementById("cap"); if (c) { c.textContent = t; c.style.display = t ? "block" : "none"; } }, t);
  const scrollTo = (sel, off = 90) => page.evaluate((sel, off) => { const e = document.querySelector(sel); if (e) window.scrollTo({ top: e.getBoundingClientRect().top + scrollY - off, behavior: "smooth" }); return !!e; }, sel, off);
  const glide = async (px, ms) => { const steps = Math.max(1, Math.round(ms / 50)); for (let i = 0; i < steps; i++) { await page.evaluate((d) => window.scrollBy(0, d), px / steps); await sleep(50); } };

  await page.goto("about:blank");
  const cdp = await page.createCDPSession();
  const frames = []; let n = 0;
  cdp.on("Page.screencastFrame", async (f) => { const file = path.join(FR, `${String(n++).padStart(6, "0")}.jpg`); fs.writeFileSync(file, Buffer.from(f.data, "base64")); frames.push({ file, ts: f.metadata.timestamp }); cdp.send("Page.screencastFrameAck", { sessionId: f.sessionId }).catch(() => {}); });
  await cdp.send("Page.startScreencast", { format: "jpeg", quality: 88, maxWidth: 1920, maxHeight: 1080, everyNthFrame: 1 });

  const scenes = [];
  let cur = "open";
  const say = () => plain(SCENES.find((s) => s.k === cur).say);
  const go = async (url, wait = "networkidle2") => { await page.goto(url, { waitUntil: wait }); await prep().catch(() => {}); await caption(say()).catch(() => {}); };
  // a scene lasts as long as its narration or its live action, whichever is longer
  const scene = async (k, fn) => {
    cur = k;
    const t = Date.now() / 1000;
    await prep().catch(() => {}); await caption(say()).catch(() => {});
    const end = t + dur[k] + 0.45;
    await fn().catch((e) => console.log("scene", k, e.message));
    await prep().catch(() => {}); await caption(say()).catch(() => {});
    while (Date.now() / 1000 < end) await sleep(100);
    scenes.push({ k, start: t, end: Date.now() / 1000 });
  };
  const thumb = pathToFileURL(path.join(ROOT, "scripts/demo/thumb.html")).href;
  const term = pathToFileURL(path.join(ROOT, "scripts/demo/term.html")).href;
  const transcript = [];
  const openTerm = async (title) => { await page.goto(term, { waitUntil: "networkidle0" }); await prep(); await caption(say()); await page.evaluate((t) => window.begin(t), title); };
  // types the command, runs it for real, and streams its stdout and stderr into the terminal as they arrive
  const live = async (prompt, shown, exe, args, opts = {}) => {
    await page.evaluate((p, c) => window.cmd(p, c), prompt, shown);
    const t0 = Date.now();
    let q = Promise.resolve();
    const push = (d) => { const s = d.toString("utf8"); transcript.push(s); q = q.then(() => page.evaluate((x) => window.w(x), s)).catch(() => {}); };
    const code = await new Promise((res) => {
      const p = spawn(exe, args, { cwd: ROOT, windowsHide: true, ...opts });
      p.stdout.on("data", push); p.stderr.on("data", push);
      p.on("close", res); p.on("error", (e) => { push(String(e) + "\n"); res(-1); });
    });
    await q;
    console.log("live", shown, "exit", code, ((Date.now() - t0) / 1000).toFixed(1) + "s");
    transcript.push(`\n[${shown}: exit ${code}, ${((Date.now() - t0) / 1000).toFixed(1)} s]\n`);
    return code;
  };
  const PS = "\x1b[32m" + ROOT + ">\x1b[0m ";
  const A = "work\\atlas.exe";

  await scene("open", async () => { await go(thumb, "load"); });
  await scene("doctor", async () => {
    await openTerm("Windows 11 host, QNX SDP 8.0 (recorded live)");
    await live(PS, A + " doctor", ATLAS, ["doctor"]);
  });
  await scene("run", async () => {
    await openTerm("atlas run and atlas probe (recorded live)");
    await live(PS, A + " run --only socket2,tokio,rustls,reqwest,ring,nix,target-lexicon --force --out work/demo-run.jsonl", ATLAS,
      ["run", "--only", "socket2,tokio,rustls,reqwest,ring,nix,target-lexicon", "--force", "--out", "work/demo-run.jsonl"]);
    await sleep(1500);
    await live(PS, A + " probe ctrlc --target aarch64-unknown-nto-qnx800", ATLAS, ["probe", "ctrlc", "--target", "aarch64-unknown-nto-qnx800"]);
    await sleep(2500);
  });
  await scene("tiles", async () => { await go(SITE + "/"); await sleep(1500); await scrollTo("#tiles", 260); await sleep(7000); await scrollTo(".caveat", 300); });
  await scene("breaks", async () => { await scrollTo("#breaks"); await sleep(4500); await scrollTo("#blockers", 200); await sleep(2500); await glide(250, 3000); });
  await scene("fixes", async () => { await scrollTo("#fixes"); await sleep(5500); await go("https://github.com/aws/aws-lc-rs/pull/1292"); await sleep(1200); await glide(500, 4000); });
  await scene("ontarget", async () => {
    await openTerm("WSL2 Ubuntu 24.04: QNX 8.0 x86_64 under QEMU/KVM (recorded live)");
    await live("\x1b[32mroot@wsl\x1b[0m# ", "bash scripts/demo/vm_live.sh " + EXAMPLES.join(" "), "wsl",
      ["-d", WSL_DISTRO, "-u", "root", "--", "bash", "/mnt/c" + ROOT.replace(/\\/g, "/").slice(2) + "/scripts/demo/vm_live.sh", ...EXAMPLES.map((e) => `${BIN}/${e}`)]);
    await sleep(4000);
    await go(SITE + "/#ontarget"); await scrollTo("#ontarget"); await glide(300, 3000);
  });
  await scene("codelab", async () => { await go(SITE + "/codelab/rust-on-qnx-windows/"); await sleep(3500); await page.evaluate(() => { const a = [...document.querySelectorAll('a[href^="#"]')].find((x) => /libc trap/i.test(x.textContent)); if (a) a.click(); }); await sleep(1500); await glide(400, 4000); });
  await scene("outro", async () => { await go(thumb + "?s2=" + encodeURIComponent("github.com/Mattbusel/qnx-crate-atlas  ·  " + SITE.replace("https://", "")), "load"); });

  await cdp.send("Page.stopScreencast"); await browser.close();
  fs.writeFileSync(path.join(OUT, "live-transcript.txt"), transcript.join(""));

  frames.sort((a, b) => a.ts - b.ts);
  const t0 = scenes[0].start, tEnd = scenes.at(-1).end;
  const use = frames.filter((f) => f.ts >= t0 - 0.05 && f.ts <= tEnd);
  const list = [];
  for (let i = 0; i < use.length; i++) { const b = i + 1 < use.length ? use[i + 1].ts : tEnd; list.push(`file '${use[i].file.replace(/\\/g, "/")}'`, `duration ${Math.max(0.001, b - use[i].ts).toFixed(4)}`); }
  list.push(`file '${use.at(-1).file.replace(/\\/g, "/")}'`);
  fs.writeFileSync(path.join(OUT, "frames.txt"), list.join("\n"));
  const args = ["-y", "-f", "concat", "-safe", "0", "-i", path.join(OUT, "frames.txt")];
  scenes.forEach((s) => args.push("-i", path.join(OUT, `${s.k}.wav`)));
  const delays = scenes.map((s, i) => `[${i + 1}:a]adelay=${Math.round((s.start - t0) * 1000)}|${Math.round((s.start - t0) * 1000)}[a${i}]`).join(";");
  const mix = scenes.map((_, i) => `[a${i}]`).join("") + `amix=inputs=${scenes.length}:normalize=0,apad[aout]`;
  const mp4 = path.join(OUT, "qnx-crate-atlas-demo.mp4");
  args.push("-filter_complex", `${delays};${mix};[0:v]scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps=30,format=yuv420p[vout]`,
    "-map", "[vout]", "-map", "[aout]", "-t", (tEnd - t0).toFixed(2), "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-c:a", "aac", "-b:a", "160k", mp4);
  const r = spawnSync("ffmpeg", args, { encoding: "utf8", maxBuffer: 1 << 26 });
  if (r.status !== 0) throw new Error("ffmpeg: " + r.stderr.slice(-2500));
  console.log("video", mp4, "seconds", (tEnd - t0).toFixed(1), "frames", use.length);
}
main().catch((e) => { console.error(e); process.exit(1); });
