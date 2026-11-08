"""Make a QNX 8.0 compatibility copy of libc for the QNX-shipped Rust toolchain.

libc 0.2.189 and newer describe QNX 8.0 as `target_os = "qnx"`, the name upstream Rust uses for the
renamed targets (x86_64-pc-qnx, aarch64-unknown-qnx). The Rust toolchain that QNX ships in SDP 8.0
(rustc 1.85.1, targets x86_64-pc-nto-qnx800 / aarch64-unknown-nto-qnx800) still reports
`target_os = "nto"`, `target_env = "nto80"`, so newer libc compiles none of its QNX 8 code and fails.

This script copies a libc source tree and teaches it the old spelling:
  * build.rs sets a new cfg `qnx8_nto80` when target_os == "nto" and target_env == "nto80"
  * every `target_os = "qnx"` predicate in src/ becomes `any(target_os = "qnx", qnx8_nto80)`
  * every `target_os = "nto"` predicate becomes `all(target_os = "nto", not(qnx8_nto80))`, so code
    meant only for QNX 7.x stops applying to QNX 8.0 (where either name now means "any QNX",
    the result is logically unchanged)
No definitions are changed, so the old toolchain gets exactly the QNX 8 items the new targets get.

usage: python make_libc_compat.py <libc-0.2.x source dir> <output dir>
Then in a project:  [patch.crates-io]  libc = { path = "<output dir>" }
"""
import pathlib
import re
import shutil
import sys

# (predicate, replacement): QNX 8 code paths open up for nto80, QNX 7 only paths close for it.
RULES = [
    ('target_os = "qnx"', 'any(target_os = "qnx", qnx8_nto80)'),
    ('target_os = "nto"', 'all(target_os = "nto", not(qnx8_nto80))'),
]


def patch_build_rs(text: str) -> str:
    edits = [
        ("    Uclibc32Time64,\n}", "    Uclibc32Time64,\n\n    /// QNX SDP 8.0 toolchain that still spells the target `target_os = \"nto\"`, `target_env = \"nto80\"`.\n    Qnx8Nto80,\n}"),
        ("    Cfg::Uclibc32Time64,\n];", "    Cfg::Uclibc32Time64,\n    Cfg::Qnx8Nto80,\n];"),
        ('            Cfg::Uclibc32Time64 => "uclibc32_time64",', '            Cfg::Uclibc32Time64 => "uclibc32_time64",\n            Cfg::Qnx8Nto80 => "qnx8_nto80",'),
        ("            Cfg::LibcDenyWarnings => (),", "            Cfg::LibcDenyWarnings => (),\n            Cfg::Qnx8Nto80 => assert_eq!(target_os, \"nto\", \"{cfg:?} set on non-nto platform\"),"),
        ("    if target_os == \"freebsd\" {\n        // Determine", "    // QNX SDP 8.0 ships rustc with the pre-rename target names.\n    if target_os == \"nto\" && target_env == \"nto80\" {\n        cfgs.push(Cfg::Qnx8Nto80);\n    }\n\n    if target_os == \"freebsd\" {\n        // Determine"),
    ]
    for old, new in edits:
        if old not in text:
            raise SystemExit(f"build.rs anchor not found: {old[:60]!r}")
        text = text.replace(old, new, 1)
    return text


def patch_src(path: pathlib.Path) -> int:
    n = 0
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    for i, line in enumerate(lines):
        code, sep, comment = line.partition("//")
        hits = sum(code.count(p) for p, _ in RULES)
        if hits:
            # placeholders so the second rule cannot rewrite the output of the first
            for k, (p, _) in enumerate(RULES):
                code = code.replace(p, f"@@{k}@@")
            for k, (_, r) in enumerate(RULES):
                code = code.replace(f"@@{k}@@", r)
            lines[i] = code + sep + comment
            n += hits
    if n:
        path.write_text("".join(lines), encoding="utf-8")
    return n


def main() -> None:
    src, out = map(pathlib.Path, sys.argv[1:3])
    if out.exists():
        shutil.rmtree(out)
    shutil.copytree(src, out, ignore=shutil.ignore_patterns("target", ".cargo-ok", ".cargo_vcs_info.json"))
    b = out / "build.rs"
    b.write_text(patch_build_rs(b.read_text(encoding="utf-8")), encoding="utf-8")
    total = 0
    for f in sorted((out / "src").rglob("*.rs")):
        k = patch_src(f)
        if k:
            print(f"{f.relative_to(out)}: {k}")
            total += k
    print(f"rewrote {total} target_os predicates")
    if not re.search(r"qnx8_nto80", (out / "build.rs").read_text(encoding="utf-8")):
        raise SystemExit("build.rs not patched")


if __name__ == "__main__":
    main()
