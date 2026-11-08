# Patches

| File | What it fixes | Status |
|---|---|---|
| `make_libc_compat.py` | libc 0.2.189+ only knows QNX 8.0 as `target_os = "qnx"`; the QNX SDP 8.0 Rust toolchain reports `target_os = "nto"`, `target_env = "nto80"`. Makes a copy of libc where build.rs sets `qnx8_nto80` for that toolchain and the `target_os` predicates are rewritten so it gets exactly the QNX 8 code paths. | Local compatibility layer. The libc maintainers chose to follow the upstream target rename ([rust-lang/libc#5341](https://github.com/rust-lang/libc/issues/5341)), so the lasting fix is a QNX toolchain with the new target names. |
| `aws-lc-rs-qnx-cc-builder.patch` | `aws-lc-sys` C build: `qcc` rejects `--include=`, and QNX headers need `_QNX_SOURCE`. | Upstream PR [aws/aws-lc-rs#1292](https://github.com/aws/aws-lc-rs/pull/1292) |
| `target-lexicon-qnx.patch` | `target-lexicon` cannot parse QNX triples, so build scripts that use it (pyo3) panic. | Upstream PR [bytecodealliance/target-lexicon#138](https://github.com/bytecodealliance/target-lexicon/pull/138) |
| `aws-lc-qnx-aarch64-cpuid.patch` | aws-lc had no QNX aarch64 CPU feature detection, so a program using rustls fails to link for aarch64 (`OPENSSL_cpuid_setup`). Applied to the aws-lc submodule of an aws-lc-rs main checkout (`work/awslc-rs-main`). | Upstream PR [aws/aws-lc#3780](https://github.com/aws/aws-lc/pull/3780) |
| `prepare.py` | Builds the patched copies under `work/` from the cargo registry cache. | |

Use them in any project with `[patch.crates-io]`, as in `examples/`, or let `atlas fix <project>` add exactly the entries your lockfile needs and verify the build.
