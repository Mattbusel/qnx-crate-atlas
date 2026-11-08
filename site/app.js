// Renders data/atlas.json (written by `atlas report`). No framework, no build step.
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const SHORT = { "x86_64-pc-nto-qnx800": "x86_64", "aarch64-unknown-nto-qnx800": "aarch64" };
const pct = (n, d) => (d ? Math.round((1000 * n) / d) / 10 : 0);
const tip = $("#tip");
function showTip(e, html) { tip.innerHTML = html; tip.hidden = false; moveTip(e); }
function moveTip(e) { const x = Math.min(e.clientX + 14, innerWidth - tip.offsetWidth - 8); tip.style.left = x + "px"; tip.style.top = e.clientY + 14 + "px"; }
function hideTip() { tip.hidden = true; }
function hover(el, html) { el.addEventListener("mouseenter", (e) => showTip(e, html)); el.addEventListener("mousemove", moveTip); el.addEventListener("mouseleave", hideTip); }

// final outcome for one crate on one target, counting the compat pass
function outcome(r) {
  if (!r) return null;
  if (r.status === "pass") return { k: "pass", label: "builds" };
  if (r.status === "pass-with-libc-pin") return { k: "pin", label: "libc pin" };
  if (r.status === "skip") return { k: "skip", label: "no lib" };
  if (r.compat && r.compat.status === "pass") return { k: "compat", label: "our patches" };
  const err = (r.compat && r.compat.error) || r.error;
  const cls = (r.compat && r.compat.class) || r.class;
  return { k: "fail", label: cls || "fails", err };
}

fetch("data/atlas.json").then((r) => r.json()).then(render).catch((e) => { $("#tiles").innerHTML = `<p>Could not load data: ${esc(e)}</p>`; });

function render(d) {
  const targets = d.targets;
  const crates = d.crates;
  $("#n-crates").textContent = d.meta?.list_size ?? crates.length;
  const rv = (d.meta?.rustc || "").split("\n")[0];
  const when = d.meta?.generated_unix ? new Date(d.meta.generated_unix * 1000).toISOString().slice(0, 10) : "";
  $("#meta").textContent = [rv && `Toolchain: ${rv} (QNX SDP 8.0), ${d.meta.cargo || ""}`, when && `Data: ${when}`].filter(Boolean).join(" · ");

  // headline tiles
  $("#tiles").innerHTML = targets.map((t) => {
    const s = d.summary[t];
    const base = s.attempted - s.skip;
    const ob = s.pass, pin = s.pass + s.pass_with_libc_pin, cp = s.pass_with_libc_compat;
    const parts = [["pass", ob], ["pin", pin - ob], ["compat", cp - pin], ["fail", base - cp]];
    return `<div class="tile"><h3>${esc(t)}</h3>
      <div class="steps">
        <div class="step"><div class="big">${pct(ob, base)}%</div><div class="lbl">build out of the box</div></div>
        <div class="step"><div class="big">${pct(pin, base)}%</div><div class="lbl">with libc 0.2.186 pin</div></div>
        <div class="step"><div class="big">${pct(cp, base)}%</div><div class="lbl">with our patches</div></div>
      </div>
      <div class="stack" data-t="${esc(t)}">${parts.map(([k, n]) => `<span class="c-${k}" data-k="${k}" data-n="${n}" style="flex:${n}"></span>`).join("")}</div>
      <div class="legend"><span><i class="c-pass"></i>builds ${ob}</span><span><i class="c-pin"></i>+ libc pin ${pin - ob}</span><span><i class="c-compat"></i>+ our patches ${cp - pin}</span><span><i class="c-fail"></i>still fails ${base - cp}</span></div>
      <div class="lbl" style="margin-top:6px;color:var(--muted);font-size:13px">${base} crates counted${s.skip ? `; ${s.skip} left out because they need a non-default feature to build anywhere` : ""}</div></div>`;
  }).join("");
  const words = { pass: "build with no changes", pin: "build once libc is pinned to 0.2.186", compat: "build only with our patches (libc compatibility copy, aws-lc-sys and target-lexicon fixes)", fail: "still fail" };
  document.querySelectorAll(".stack span").forEach((el) => hover(el, `<b>${el.dataset.n}</b> crates ${words[el.dataset.k]}`));

  // classes
  const desc = Object.fromEntries(d.classes.map((c) => [c.slug, c.description]));
  $("#class-defs").innerHTML = d.classes.map((c) => `<dt>${esc(c.slug)}</dt><dd>${esc(c.description)}</dd>`).join("");
  const seg = $("#class-target");
  const drawClasses = (t) => {
    seg.querySelectorAll("button").forEach((b) => b.setAttribute("aria-selected", b.dataset.t === t));
    const m = d.summary[t].pass1_by_class;
    const rows = Object.entries(m).sort((a, b) => b[1] - a[1]);
    const max = Math.max(1, ...rows.map((r) => r[1]));
    const el = $("#classes");
    el.innerHTML = rows.map(([k, n]) => `<div class="hrow" data-k="${esc(k)}"><div class="name">${esc(k)}</div><div class="track"><div class="fill" style="width:${(100 * n) / max}%"></div></div><div class="val">${n}</div></div>`).join("") || "<p>No failures.</p>";
    el.querySelectorAll(".hrow").forEach((r) => {
      const k = r.dataset.k;
      const ex = crates.filter((c) => { const x = c.results[t]; return x && (x.pass1_class || (x.status === "fail" ? x.class : null)) === k; }).slice(0, 6).map((c) => c.name);
      hover(r, `<b>${esc(k)}</b>: ${m[k]} crates<br>${esc(desc[k] || "")}<br><span style="color:var(--muted)">e.g. ${esc(ex.join(", "))}</span>`);
    });
  };
  seg.innerHTML = targets.map((t) => `<button role="tab" data-t="${esc(t)}">${esc(SHORT[t] || t)}</button>`).join("");
  seg.querySelectorAll("button").forEach((b) => b.addEventListener("click", () => drawClasses(b.dataset.t)));
  drawClasses(targets[0]);

  // blockers after every fix, across both targets
  const bl = new Map();
  for (const c of crates) for (const t of targets) {
    const o = outcome(c.results[t]);
    if (!o || o.k !== "fail") continue;
    const p0 = o.err && o.err.package;
    const pkg = p0 && p0 !== "atlas-probe" ? p0 : c.name;
    const e = bl.get(pkg) || { pkg, crates: new Set(), cls: new Map(), msg: o.err?.message || "" };
    e.crates.add(c.name);
    e.cls.set(o.label, (e.cls.get(o.label) || 0) + 1);
    bl.set(pkg, e);
  }
  const blr = [...bl.values()].sort((a, b) => b.crates.size - a.crates.size || a.pkg.localeCompare(b.pkg)).slice(0, 15);
  const bmax = Math.max(1, ...blr.map((b) => b.crates.size));
  $("#blockers").innerHTML = `<table><thead><tr><th>Blocking crate</th><th>Top crates blocked</th><th>Class</th><th>Error</th></tr></thead><tbody>${blr.map((b) => {
    const cls = [...b.cls.entries()].sort((x, y) => y[1] - x[1])[0][0];
    const ex = [...b.crates].slice(0, 5).join(", ") + (b.crates.size > 5 ? ", ..." : "");
    return `<tr><td><span class="cr">${esc(b.pkg)}</span></td><td><span class="minibar" style="width:${(90 * b.crates.size) / bmax}px"></span>${b.crates.size}<div class="ver">${esc(ex)}</div></td><td>${esc(cls)}</td><td><span class="err">${esc(b.msg)}</span></td></tr>`;
  }).join("")}</tbody></table>`;

  // on target
  const ot = d.ontarget || [];
  if (!ot.length) { $("#ontarget-table").innerHTML = "<p style='padding:12px'>On-target run not published yet.</p>"; }
  else {
    const ran = ot.filter((x) => x.stage === "ran" && x.passed + x.failed > 0);
    const tp = ran.reduce((a, x) => a + x.passed, 0), tf = ran.reduce((a, x) => a + x.failed, 0);
    const clean = ran.filter((x) => x.failed === 0 && x.exit_codes.every((c) => c === "0")).length;
    const un = ran.find((x) => x.uname)?.uname;
    $("#ot-sub").innerHTML += ` Of <b>${ran.length}</b> crates with unit tests, <b>${clean}</b> passed every test; <b>${tp.toLocaleString()}</b> tests passed, <b>${tf}</b> failed.` + (un ? `<br><code>${esc(un)}</code>` : "");
    $("#ontarget-table").innerHTML = `<table><thead><tr><th class="num">#</th><th>Crate</th><th class="num">Passed</th><th class="num">Failed</th><th class="num">Ignored</th><th>Result</th></tr></thead><tbody>${ot.map((x) => {
      let res;
      if (x.stage === "ran" && x.passed + x.failed === 0) res = `<span class="badge b-skip">ran, 0 tests</span>`;
      else if (x.stage === "ran") res = x.failed === 0 && x.exit_codes.every((c) => c === "0") ? `<span class="badge b-pass">all tests pass</span>` : `<span class="badge b-fail">${x.failed} failed</span> <span class="err">${esc(x.failed_tests.slice(0, 3).join(", "))}${x.exit_codes.includes("timeout") ? " (timeout)" : ""}${x.crate_name === "rustix" ? " (these tests open src/buffer.rs, which is not on the VM)" : ""}</span>`;
      else if (x.stage === "no-tests") res = `<span class="badge b-skip">no QNX test binary</span> <span class="err">no lib unit tests, or a proc-macro (tests run on the host)</span>`;
      else res = `<span class="badge b-skip">tests did not build</span> <span class="err">${esc(x.build_error || "")}</span>`;
      return `<tr><td class="num">${x.rank}</td><td><span class="cr">${esc(x.crate_name)}</span> <span class="ver">${esc(x.version)}</span></td><td class="num">${x.stage === "ran" ? x.passed : ""}</td><td class="num">${x.stage === "ran" ? x.failed : ""}</td><td class="num">${x.stage === "ran" ? x.ignored : ""}</td><td>${res}</td></tr>`;
    }).join("")}</tbody></table>`;
  }

  // full grid
  const tbody = $("#grid tbody");
  const cell = (o) => o ? `<span class="badge b-${o.k === "compat" ? "pin" : o.k}">${esc(o.label)}</span>` : "";
  const rows = crates.map((c) => {
    const os = targets.map((t) => outcome(c.results[t]));
    const r0 = c.results[targets[0]] || {};
    const f = os.find((o) => o && o.k === "fail");
    let note = "";
    if (f && f.err) note = `<span class="err">${esc(f.err.package && f.err.package !== c.name ? f.err.package + ": " : "")}${esc(f.err.code ? f.err.code + " " : "")}${esc(f.err.message)}</span>`;
    else if (os.some((o) => o && (o.k === "pin" || o.k === "compat"))) note = `<span class="err">libc ${esc(r0.pass1_class || "")} on pass 1</span>`;
    const kind = f ? "fail" : os.some((o) => o && (o.k === "pin" || o.k === "compat")) ? "pin" : "pass";
    const text = [c.name, c.description, note].join(" ").toLowerCase();
    return { kind, text, html: `<tr><td class="num">${c.rank}</td><td><span class="cr">${esc(c.name)}</span> <span class="ver">${esc(r0.version || c.latest)}</span></td><td>${cell(os[0])}</td><td>${cell(os[1])}</td><td>${note}</td></tr>` };
  });
  let limit = 100;
  const draw = () => {
    const q = $("#q").value.trim().toLowerCase(), f = $("#f").value;
    const shown = rows.filter((r) => (!f || r.kind === f) && (!q || r.text.includes(q)));
    tbody.innerHTML = shown.slice(0, limit).map((r) => r.html).join("");
    const more = shown.length > limit ? ` <button id="more" class="more">Show all ${shown.length}</button>` : "";
    $("#grid-count").innerHTML = `${Math.min(limit, shown.length)} of ${shown.length} matching crates (${rows.length} in the atlas)${more}`;
    $("#more")?.addEventListener("click", () => { limit = Infinity; draw(); });
  };
  $("#q").addEventListener("input", () => { limit = 100; draw(); }); $("#f").addEventListener("change", () => { limit = 100; draw(); });
  draw();
}
