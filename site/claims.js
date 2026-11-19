// Renders data/claims.json (written by `atlas claims-atlas`): crates whose no_std, embedded, QNX or
// portable claim does not build. No framework, no build step.
(function () {
  const $ = (s) => document.querySelector(s);
  const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
  const fmt = (n) => Number(n || 0).toLocaleString("en-US");
  const claimText = (c) => (c ? `${c.file}${c.line ? ":" + c.line : ""} ${c.text}` : "");
  const errText = (fe) => {
    if (!fe || !fe.message) return "";
    const code = fe.code ? `error[${fe.code}]` : "error";
    const at = fe.file ? ` at ${fe.package}-${fe.version}/${fe.file}${fe.line ? ":" + fe.line : ""}` : fe.package ? ` in ${fe.package} ${fe.version}` : "";
    return `${code}: ${fe.message}${at}`;
  };
  const blameText = (b, fe) => (b === "crate" ? "its own code" : b === "dependency" ? `dependency ${fe?.package || ""}` : b === "toolchain" ? "toolchain" : "");

  fetch("data/claims.json")
    .then((r) => r.json())
    .then(render)
    .catch(() => {
      const s = $("#claims");
      if (s) s.hidden = true;
    });

  function render(d) {
    const t = d.totals;
    const checks = t.claimed_checks || {};
    const bm = checks["bare-metal"] || {};
    const qx = checks["qnx"] || {};
    const qu = t.qnx_unclaimed || {};
    const tile = (big, label, cls) => `<div class="tile"><div class="big ${cls || ""}">${big}</div><p>${label}</p></div>`;
    $("#claims-tiles").innerHTML = [
      tile(fmt(t.crates_with_claims), `of the top ${fmt(t.crates_scanned)} crates claim no_std, embedded, QNX or portable support`),
      tile(fmt(t.crates_where_a_claim_fails_to_compile), "of them have a claim that does not compile on the target it names", "bad"),
      tile(fmt(t.crates_that_refuse_the_target_on_purpose), "more stop the build on purpose with compile_error! (the claim needs an opt-in the docs mention)", "warn"),
      tile(`${fmt(bm.fail)} / ${fmt((bm.pass || 0) + (bm.fail || 0))}`, `no_std / embedded claims that fail on <code>${esc(d.meta.bare_metal_target)}</code>`, "bad"),
      tile(`${fmt(qx.fail)} / ${fmt((qx.pass || 0) + (qx.fail || 0))}`, `QNX / portable claims that fail on <code>${esc(d.meta.qnx_target)}</code>`, "bad"),
      tile(`${fmt(qu.fail)}`, `crates with no claim that also fail on <code>${esc(d.meta.qnx_target)}</code> (they never promised)`, ""),
    ].join("");
    const when = d.meta.generated_unix ? new Date(d.meta.generated_unix * 1000).toISOString().slice(0, 10) : "";
    $("#claims-meta").textContent = [d.meta.stable, d.meta.nightly, when && `Data: ${when}`].filter(Boolean).join(" · ");

    const rows = (d.broken || []).slice().sort((a, b) => a.rank - b.rank);
    $("#claims-count").textContent = `${rows.length} broken claims across ${t.crates_with_a_broken_claim} crates. Download claims.json for the cargo tree path behind every dependency failure.`;
    $("#claims-table").innerHTML = `<table class="grid"><thead><tr><th class="num">#</th><th>Crate</th><th>What it claims (file:line)</th><th>Target</th><th>Why it fails</th><th>First error</th></tr></thead><tbody>${rows
      .map(
        (r) => `<tr class="${r.reason === "explicit-refusal" ? "refuse" : ""}"><td class="num">${r.rank}</td><td><b>${esc(r.crate_name)}</b> ${esc(r.version)}</td><td><code>${esc(claimText(r.claim))}</code></td><td><code>${esc(r.target)}</code><br><span class="sub">${esc(r.features)}</span></td><td>${esc(r.reason)}${r.blame ? `, ${esc(blameText(r.blame, r.first_error))}` : ""}</td><td><code>${esc(errText(r.first_error))}</code></td></tr>`
      )
      .join("")}</tbody></table>`;
    const bl = (d.blockers || []).filter((b) => b.broken_claims > 1).slice(0, 12);
    $("#claims-blockers").innerHTML = bl.length
      ? `<table><thead><tr><th>Package where the build stops</th><th class="num">Broken claims</th><th>Crates whose claim it breaks</th></tr></thead><tbody>${bl
          .map((b) => `<tr><td><code>${esc(b.package)}</code></td><td class="num">${b.broken_claims}</td><td>${esc(b.crates.join(", "))}</td></tr>`)
          .join("")}</tbody></table>`
      : "";
  }
})();
