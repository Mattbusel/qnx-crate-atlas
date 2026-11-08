"""Headline numbers from site/data/atlas.json, as Markdown (for README / Devpost) and JSON (for the video).
usage: python scripts/numbers.py [--json out.json]"""
import json, pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
d = json.loads((ROOT / "site/data/atlas.json").read_text(encoding="utf-8"))
T = {"x86_64-pc-nto-qnx800": "x86", "aarch64-unknown-nto-qnx800": "arm"}
pct = lambda n, b: round(100 * n / b, 1) if b else 0.0
nums = {"crates": d["meta"].get("list_size", len(d["crates"]))}
rows = []
for t in d["targets"]:
    s = d["summary"][t]
    base = s["attempted"] - s["skip"]
    ob, pin, cp = s["pass"], s["pass"] + s["pass_with_libc_pin"], s["pass_with_libc_compat"]
    k = T[t]
    nums.update({f"{k}_base": base, f"{k}_oob": pct(ob, base), f"{k}_pin": pct(pin, base), f"{k}_compat": pct(cp, base),
                 f"{k}_oob_n": ob, f"{k}_pin_n": pin, f"{k}_compat_n": cp, f"{k}_fail_n": base - cp})
    rows.append(f"| `{t}` | {base} | {ob} ({pct(ob, base)}%) | {pin} ({pct(pin, base)}%) | {cp} ({pct(cp, base)}%) | {base - cp} |")
# crates whose QNX test binaries ran and contain at least one test
ot = [x for x in (d.get("ontarget") or []) if x["stage"] == "ran" and x["passed"] + x["failed"] > 0]
nums["ot_ran"] = len(ot)
nums["ot_clean"] = sum(1 for x in ot if x["failed"] == 0 and all(c == "0" for c in x["exit_codes"]))
nums["ot_passed"] = sum(x["passed"] for x in ot)
nums["ot_failed"] = sum(x["failed"] for x in ot)
p1 = d["summary"]["x86_64-pc-nto-qnx800"]["pass1_by_class"]
nums["libc_cfg_x86"] = p1.get("libc-qnx8-cfg", 0)

md = ["| Target | Crates with a library | Build out of the box | + libc 0.2.186 pin | + our patches | Still fail |", "|---|---|---|---|---|---|", *rows]
if ot:
    md.append("")
    md.append(f"On a QNX 8.0 x86_64 kernel (QEMU/KVM), the unit tests of {len(ot)} of the top crates ran: {nums['ot_clean']} crates passed every test; {nums['ot_passed']:,} tests passed and {nums['ot_failed']} failed (all 4 in rustix, which reads its own source file `src/buffer.rs`, not copied to the VM).")
classes = "\n".join(f"- `{c['slug']}`: {c['description']}" for c in d["classes"])
if "--json" in sys.argv:
    pathlib.Path(sys.argv[sys.argv.index("--json") + 1]).write_text(json.dumps(nums, indent=1))
print(json.dumps(nums))
(ROOT / "work").mkdir(exist_ok=True)
(ROOT / "work/results.md").write_text("\n".join(md), encoding="utf-8")
(ROOT / "work/classes.md").write_text(classes, encoding="utf-8")
