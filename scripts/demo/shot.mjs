// Screenshot a page with headless Chrome: node scripts/demo/shot.mjs <url> <out.png> [width] [dark]
import puppeteer from "puppeteer-core";
const [url, out, w = "1280", dark] = process.argv.slice(2);
const b = await puppeteer.launch({ executablePath: "C:/Program Files/Google/Chrome/Application/chrome.exe", headless: "new", args: ["--no-first-run"] });
const p = await b.newPage();
await p.setViewport({ width: +w, height: 900 });
if (dark) await p.emulateMediaFeatures([{ name: "prefers-color-scheme", value: "dark" }]);
p.on("console", (m) => console.log("console:", m.text()));
p.on("pageerror", (e) => console.log("pageerror:", e.message));
await p.goto(url, { waitUntil: "networkidle0" });
const H = +(process.env.SHOT_H || 0); await p.screenshot(H ? { path: out, clip: { x: 0, y: +(process.env.SHOT_Y || 0), width: +w, height: H } } : { path: out, fullPage: true });
await b.close();
