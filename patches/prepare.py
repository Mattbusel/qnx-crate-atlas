"""Create the patched dependency copies the examples and `atlas compat` use, under work/:
  work/libc-qnx8        libc 0.2.191 with the QNX 8 compatibility cfg (make_libc_compat.py)
  work/aws-lc-sys-qnx   aws-lc-sys 0.47.0 with the qcc fix sent upstream as aws/aws-lc-rs#1292
  work/target-lexicon-qnx  target-lexicon 0.13.5 with QNX triples, sent upstream as bytecodealliance/target-lexicon#138
  work/awslc-rs-main     aws-lc-rs main (has #1292) with aws-lc-qnx-aarch64-cpuid.patch (aws/aws-lc#3780) in its
                         aws-lc submodule, so rustls links for aarch64 QNX; `atlas fix` prefers it over aws-lc-sys-qnx
The sources come from the cargo registry cache; run `cargo fetch` in examples/ first if they are missing.
usage: python patches/prepare.py
"""
import os, pathlib, shutil, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
HOME = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))


def find(name):
    for idx in (HOME / "registry" / "src").glob("*"):
        p = idx / name
        if (p / "Cargo.toml").is_file():
            return p
    sys.exit(f"{name} not in {HOME / 'registry' / 'src'}; run `cargo fetch` in examples/rustls-aws-lc first")


def main():
    work = ROOT / "work"
    work.mkdir(exist_ok=True)
    subprocess.run([sys.executable, str(ROOT / "patches" / "make_libc_compat.py"), str(find("libc-0.2.191")), str(work / "libc-qnx8")], check=True)
    dst = work / "aws-lc-sys-qnx"
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(find("aws-lc-sys-0.47.0"), dst)
    f = dst / "builder" / "cc_builder.rs"
    s = f.read_text(encoding="utf-8")
    edits = [
        ('if is_cl_like { "/FI" } else { "--include=" },', 'if is_cl_like { "/FI" } else { "-include" },'),
        ('let force_include_option = if is_cl_like { "/FI" } else { "--include=" };', 'let force_include_option = if is_cl_like { "/FI" } else { "-include" };'),
        ('            } else if target_vendor() != "apple" {\n                // Needed by illumos',
         '            } else if target_os() == "nto" || target_os() == "qnx" {\n                build_options.push(BuildOption::define("_QNX_SOURCE", "1"));\n            } else if target_vendor() != "apple" {\n                // Needed by illumos'),
    ]
    s = s.replace("\r\n", "\n")
    for old, new in edits:
        if old not in s:
            sys.exit(f"aws-lc-sys anchor not found: {old[:50]!r}")
        s = s.replace(old, new, 1)
    f.write_text(s, encoding="utf-8")
    print(f"patched {dst}")
    tl = work / "target-lexicon-qnx"
    if tl.exists():
        shutil.rmtree(tl)
    shutil.copytree(find("target-lexicon-0.13.5"), tl)
    # its own repository, so `git apply` resolves paths here and not in an enclosing checkout
    subprocess.run(["git", "init", "-q"], cwd=tl, check=True)
    subprocess.run(["git", "apply", "-p1", "--ignore-whitespace", str(ROOT / "patches" / "target-lexicon-qnx.patch")], cwd=tl, check=True)
    if "Qnx800" not in (tl / "src" / "targets.rs").read_text(encoding="utf-8"):
        sys.exit("target-lexicon patch did not apply")
    print(f"patched {tl}")
    rs = work / "awslc-rs-main"
    if not rs.exists():
        subprocess.run(["git", "clone", "-q", "--depth", "1", "--recurse-submodules", "--shallow-submodules", "https://github.com/aws/aws-lc-rs.git", str(rs)], check=True)
    sub = rs / "aws-lc-sys" / "aws-lc"
    p = ROOT / "patches" / "aws-lc-qnx-aarch64-cpuid.patch"
    if not (sub / "crypto" / "fipsmodule" / "cpucap" / "cpu_aarch64_qnx.c").is_file():
        subprocess.run(["git", "apply", "-p1", "--ignore-whitespace", str(p)], cwd=sub, check=True)
    print(f"patched {rs}")


if __name__ == "__main__":
    main()
