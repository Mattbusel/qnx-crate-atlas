# QNX Crate Atlas

**Your crate says it is portable. The compiler says it is not.**

crates.io lets any crate put `no-std` in its categories, `embedded` in its keywords and "cross-platform" in its README, and nothing checks. `atlas claims` checks. It reads every portability claim a crate makes (categories, keywords, description, `#![no_std]`, README and crate docs, each with its file and line), builds the crate for a target that tests that claim, and **fails with the exact first error** when the claim does not build.

Live atlas: **https://qnx-crate-atlas.vercel.app** · Codelab: **https://qnx-crate-atlas.vercel.app/codelab/** · Video: **https://youtu.be/KDCN-wQVtu4**

```console
cargo install qnx-crate-atlas
atlas claims .                                  # your crate; exit 1 if a claim does not build
atlas claims --crate memmap2                    # any published crate
```

```yaml
# .github/workflows/claims.yml: fail the PR that breaks a claim, with the error on the claim's line
- uses: Mattbusel/qnx-crate-atlas@v0.2.0
```

## The top 500, held to their own claims

Top 500 crates on crates.io by all-time downloads, November 19 2026. no_std and embedded claims are built for `thumbv7em-none-eabihf` (stable 1.100). QNX and "portable" claims are built for upstream Rust's QNX 8.0 target `x86_64-pc-qnx` (nightly, `-Zbuild-std`). Full data: [`data/claims.jsonl`](data/claims.jsonl), summary [`site/data/claims.json`](site/data/claims.json).

| | Crates |
|---|---|
| Make a no_std, embedded, QNX or portable claim | **321** of 500 |
| Have a claim that **does not compile** on the target it names | **8** |
| Stop the build on purpose with `compile_error!` (the claim needs an opt-in) | 6 more |
| Fail on upstream QNX without claiming anything | 39 |

The ones that do not compile:

| # | Crate | The claim | Target | First error |
|---|---|---|---|---|
| 33 | rustix 1.1.6 | README.md:45 "is portable to many OS's" | `x86_64-pc-qnx` | `src/backend/libc/io/errno.rs:253` E0425 `EDOTDOT`: every QNX cfg says `target_os = "nto"`. Fix: [bytecodealliance/rustix#1750](https://github.com/bytecodealliance/rustix/pull/1750) |
| 141 | redox_syscall 0.9.5 | src/lib.rs:1 `cfg_attr(not(any(feature = "std", test)), no_std)` | `thumbv7em-none-eabihf` | `src/sigabi.rs:165` E0432 `AtomicU64` (no 64-bit atomics on Cortex-M) |
| 160 | anstyle-query 1.1.5 | Cargo.toml:37 `keywords = ["no-std"]` | `thumbv7em-none-eabihf` | `src/lib.rs:1` E0463 can't find crate for `std`. Fix: [rust-cli/anstyle#376](https://github.com/rust-cli/anstyle/pull/376) |
| 279 | addr2line 0.27.1 | Cargo.toml:32 "A cross-platform symbolication library" | `x86_64-pc-qnx` | memmap2 `src/unix.rs:476` E0425 `madvise` |
| 345 | memmap2 0.9.12 | Cargo.toml:28 "Cross-platform Rust API for memory-mapped file IO" | `x86_64-pc-qnx` | `src/unix.rs:476` E0425 `madvise` (QNX only has `posix_madvise`). Fix: [RazrFalcon/memmap2-rs#182](https://github.com/RazrFalcon/memmap2-rs/pull/182) |
| 415 | pest_meta 2.9.6 | _README.md:236 "## no_std support" | `thumbv7em-none-eabihf` | pest_stack_guard `src/lib.rs:1` E0463 `std` |
| 416 | pest_generator 2.9.6 | _README.md:236 "## no_std support" | `thumbv7em-none-eabihf` | pest_stack_guard `src/lib.rs:1` E0463 `std` |
| 470 | alloc-stdlib 0.3.0 | Cargo.toml:26 `keywords = ["nostd"]` | `thumbv7em-none-eabihf` | `src/lib.rs:1` E0463 can't find crate for `std` |

Refusals (getrandom, rustix's no_std mode through errno, cpufeatures, errno, ring and sct through getrandom) are listed separately on the site: they fail, but they say why. getrandom alone stops 13 of the top 500 (itself included) on upstream QNX, because its `use_file` backend only matches `target_os = "nto"`.

### How a claim is judged

- A no_std claim gets the most generous no_std setup the crate offers: `default-features = false`, then its own `alloc` / `no_std` / `libm` features (and any feature named in `cfg_attr(feature = "...", no_std)`), then default features. It fails only if none of them builds.
- Not a claim: `#![no_std]` followed by an unconditional `extern crate std;` (a prelude trick), "bare-metal performance", "portable SIMD", README lists of other projects, negated sentences ("does **not** support no_std").
- Not decidable on the host, so a warning, not a failure (`--strict` makes it one): a build script that needs a C cross compiler (`arm-none-eabi-gcc`, `qcc`) or configuration (`PYO3_CROSS_PYTHON_VERSION`), a crate that needs a feature choice, or one that needs nightly.
- OS-specific crates (`#![cfg(windows)]`, `cfg_attr(unix, no_std)`, `os::windows-apis`) are not built for bare metal.
- When a dependency is to blame, the report includes the `cargo tree -i -e features` path that turned `std` on, so you know which `default-features = false` you forgot.

```text
broken-no-std 0.1.0: 5 portability claim(s)
  claim no-std    Cargo.toml:8 categories = ["no-std"]
  FAIL bare-metal thumbv7em-none-eabihf [stable] with default-features = false: needs-std, in a dependency
       error[E0463]: can't find crate for `std` at memchr-2.8.3/src/lib.rs:198
       pulled in by (cargo tree -i, with features):
         memchr v2.8.3
         |-- memchr feature "alloc"
         |   `-- memchr feature "std"
         |       `-- memchr feature "default"
         |           `-- broken-no-std v0.1.0
```

[`examples/claims`](examples/claims) has an honest and a broken crate; CI runs both on every push.

## The QNX 8.0 atlas

**Which of the 500 most downloaded Rust crates build for QNX 8.0 with the toolchain QNX ships, what breaks the ones that do not, and the fixes.**

QNX ships a Rust compiler for QNX 8.0 in the QNX Software Center. Nobody had measured how much of the crates.io ecosystem actually builds with it. The Atlas does that with the real QNX toolchain: it cross-compiles every crate in the top 500 for both QNX 8.0 targets on a Windows host, links each one into a QNX binary with `qcc`, records the first error of every failure, sorts it into a class that maps to a fix, and runs crate unit tests on a QNX 8.0 kernel.

Results (QNX SDP 8.0 Rust toolchain, rustc 1.85.1, top 500 crates by all-time downloads as of November 19 2026, default features):

| Target | Crates with a library | Build out of the box | + libc 0.2.186 pin | + our patches | Still fail |
|---|---|---|---|---|---|
| `x86_64-pc-nto-qnx800` | 498 | 422 (84.7%) | 460 (92.4%) | 473 (95.0%) | 25 |
| `aarch64-unknown-nto-qnx800` | 498 | 422 (84.7%) | 460 (92.4%) | 472 (94.8%) | 26 |

A pass means the crate compiles for the target and links into a QNX executable that depends on it but does not call it. That is a weaker test than real use in one known case: on aarch64, 4 crates that bring in aws-lc-rs through rustls (`rustls`, `tokio-rustls`, `hyper-rustls`, `reqwest`) pass the probe, but a program that actually uses rustls fails to link there (`OPENSSL_cpuid_setup` is undefined). Counting those 4 as failures, aarch64 with our patches is 468 (94.0%).

On a QNX 8.0 x86_64 kernel (QEMU/KVM), the unit tests of 19 of the top crates ran: 18 crates passed every test; 1,545 tests passed and 4 failed (all 4 in rustix, which reads its own source file `src/buffer.rs`, not copied to the VM).

## Fix your own project: `atlas fix`

The fixes the atlas measured can be applied to any Cargo project in one command:

```
atlas fix path/to/your-project --before
```

It reads the project's `Cargo.lock`, and only for crates actually in the graph it adds `[patch.crates-io]` entries pointing at the patched copies made by `python patches/prepare.py` (libc compatibility copy for libc 0.2.189+, aws-lc-sys with the qcc fix and the aarch64 CPU detection fix, target-lexicon with QNX triples). It writes `.cargo/config.toml` settings for the QNX toolchain (MSRV-aware resolution for rustc 1.85; on Windows hosts, `incremental = false` and `codegen-units = 16` so `qcc` does not hit the command-line limit when linking large graphs), re-resolves the lockfile if a dependency needs a newer rustc, prints every change, keeps a backup of `Cargo.toml`, and then builds the project for `x86_64-pc-nto-qnx800` and `aarch64-unknown-nto-qnx800`. `--before` also builds first, `--dry-run` only prints the plan, `--libc-pin` pins libc 0.2.186 instead of patching it.

Demo on three real projects ([`examples/fix-demos`](examples/fix-demos), full output in each `atlas-fix-output.txt`):

| Project | Before | After `atlas fix` |
|---|---|---|
| axum 0.8 + tokio (full) + serde_json HTTP API | fails both targets (libc cfg) | builds both targets |
| reqwest 0.13 (rustls, aws-lc-rs) + tokio client | fails both targets (dependency needs rustc 1.88) | builds both targets |
| clap + tracing + socket2 keepalive CLI | fails both targets (libc cfg) | builds both targets |

These were built, not run on the VM. Crates with no known fix stay failing and are reported as such: for example `nix` (no QNX cfg arm) and `ring`'s `SystemRandom` when used for real (reqwest 0.12 with `rustls-tls`).

## What it found

1. **One rename is the biggest single cause.** Of the 76 crates that fail out of the box on x86_64, 59 stop inside libc. Upstream Rust renamed the QNX 8.0 targets (`x86_64-pc-qnx`, `target_os = "qnx"`), and libc 0.2.189+ follows the new name. The toolchain QNX ships (rustc 1.85.1) still reports `target_os = "nto"`, `target_env = "nto80"`, so the newest libc compiles none of its QNX 8 code and every crate that pulls it in fails before its own code is reached.
2. **Pinning libc 0.2.186 recovers most of them**, but 0.2.186 predates QNX 8 definitions added later. The biggest casualty is `socket2` (`libc::TCP_KEEPIDLE`), which takes `tokio` networking, `hyper` clients, `reqwest`, `axum` and `tonic` down with it.
3. **`patches/make_libc_compat.py`** makes a compatibility copy of current libc that maps the old spelling onto the new one (a build-script cfg plus a mechanical rewrite of the `target_os` predicates; no definitions change). With it, `socket2` builds for both targets and TCP keepalive works on a real QNX 8.0 kernel, and `tokio` with every feature turned on (`net` included) builds for both targets and accepts a TCP connection on QNX 8.0 ([`examples/tokio-net`](examples/tokio-net)).
4. **pyo3 was blocked by target-lexicon**, which could not parse any QNX triple (`Invalid target name: 'x86_64-pc-nto-qnx800'`). The fix adds the QNX operating systems and environments: **[bytecodealliance/target-lexicon#138](https://github.com/bytecodealliance/target-lexicon/pull/138)**.
5. **rustls was blocked by its C crypto build.** `aws-lc-sys` passed `--include=` (which `qcc` rejects) and needed `_QNX_SOURCE`. The fix is upstream as **[aws/aws-lc-rs#1292](https://github.com/aws/aws-lc-rs/pull/1292)**; with it a rustls binary builds and runs on QNX 8.0 x86_64. On aarch64 a program that uses rustls still fails to link (aws-lc's aarch64 CPU feature detection had no QNX variant, undefined `OPENSSL_cpuid_setup`); the fix is open as **[aws/aws-lc#3780](https://github.com/aws/aws-lc/pull/3780)**, and with it the binary links for aarch64.

## Evidence of QNX use (where to look)

| What | Where |
|---|---|
| Drives the QNX SDP 8.0 toolchain: finds `QNX_HOST`/`QNX_TARGET`, the QNX `rustc`, `qcc`, the license, reads the nto target cfg | [`src/qnxenv.rs`](src/qnxenv.rs), `atlas doctor` |
| Builds and links every crate for `x86_64-pc-nto-qnx800` and `aarch64-unknown-nto-qnx800` with `qcc` | [`src/probe.rs`](src/probe.rs) |
| Classifies QNX failure modes (libc cfg rename, missing libc bindings, no QNX cfg arm, qcc C builds, link) | [`src/classify.rs`](src/classify.rs) (unit tested) |
| Runs crate unit tests on a QNX 8.0 VM (QEMU/KVM in WSL2, image from `mkqnximage`) | [`src/ontarget.rs`](src/ontarget.rs), [`scripts/ontarget_session.sh`](scripts/ontarget_session.sh) |
| Raw results for every crate and target, plus the run log | [`data/`](data) |
| Fixes for QNX: libc compatibility cfg, aws-lc-sys qcc fix and target-lexicon QNX triples (both sent upstream) | [`patches/`](patches) |
| Programs proven on QNX 8.0 (socket2 keepalive, tokio with all features, rustls + aws-lc-rs) | [`examples/`](examples) |
| Codelab, in the qnx.github.io codelab format | [`codelab/`](codelab) |

## Build and run

Tested with: **QNX SDP 8.0** (non-commercial license), **QNX Rust toolchain** package `com.qnx.qnx800.host.win.x86_64.rust` (rustc 1.85.1), **cargo 1.85.0** from rustup, Windows 11 host. On-target runs: **QNX 8.0 x86_64 under QEMU/KVM** in WSL2 Ubuntu 24.04, image built with `mkqnximage --type=qemu --arch=x86_64`. aarch64 is compile and link only.

```console
rustup toolchain install 1.85.0 --profile minimal
cargo build --release                 # the Atlas itself builds with any recent stable Rust
target\release\atlas doctor           # finds the SDP at %USERPROFILE%\qnx\qnx800 or $QNX_BASE
target\release\atlas probe tokio --target aarch64-unknown-nto-qnx800
```

Full reproduction of the published data:

```console
atlas top --count 500                         # data/top.json from the crates.io API
atlas run --jobs 2 --keep-logs                # pass 1 and the libc pin pass, data/results.jsonl
python patches/prepare.py                     # work/libc-qnx8 and work/aws-lc-sys-qnx
atlas compat --libc work/libc-qnx8 --patch aws-lc-sys=work/aws-lc-sys-qnx --patch target-lexicon=work/target-lexicon-qnx
atlas ontarget --limit 40                     # unit tests on the QNX 8.0 VM (needs scripts/ontarget_session.sh setup)
atlas report                                  # site/data/atlas.json
```

Builds run one at a time at idle priority with `--jobs` rustc jobs, and share a per-target build cache so common dependencies compile once. A full run of 500 crates on both targets took about 3 hours on a 24-thread PC at `--jobs 2`.

The on-target stage expects the VM described in the codelab: SDP 8.0 Linux host at `/root/qnx800` inside WSL, a VM directory at `/root/qnx-vm-x86_64` made by `mkqnximage`, and an SSH key at `/root/.ssh/id_ed25519`.

### Examples

```console
python patches\prepare.py
cd examples\socket2-keepalive
call %USERPROFILE%\qnx\qnx800\qnxsdp-env.bat
set RUSTC=%QNX_HOST%\usr\bin\rustc.exe
cargo +1.85.0 build --target x86_64-pc-nto-qnx800
```

Output on the QNX 8.0 VM:

```
socket2 keepalive on QNX: true, pid 528405
tokio net on QNX: accepted 127.0.0.1:15585
rustls + aws-lc-rs on QNX: 9 cipher suites, first TLS13_AES_256_GCM_SHA384, random byte 60
```

## How the probe works

For each crate the Atlas writes a throwaway binary package that depends on it with default features (`probe_dep = { package = "<crate>", version = "*" }`, `extern crate probe_dep;`) and runs `cargo build --target <qnx target> --message-format=json`. That compiles the crate and all its dependencies, runs build scripts (including C builds through `qcc`), and links a QNX executable. The probe does not call the crate, so a C library the crate wraps is only linked if something references it; see the aarch64 rustls note under the results. Cargo's MSRV-aware resolver picks the newest versions that declare support for rustc 1.85, so the result reflects what a QNX user can actually use today.

1. **Pass 1** builds the graph as resolved.
2. **Pass 2** runs when pass 1 failed and libc 0.2.189+ is in the graph: `cargo update -p libc --precise 0.2.186`, build again.
3. **Pass 3** (`atlas compat`) swaps in our patched copies with `[patch.crates-io]`: the libc compatibility copy, aws-lc-sys with the qcc fix, and target-lexicon with QNX triples (`python patches/prepare.py` makes all three).

The first error comes from cargo's JSON diagnostics (or the build script output) and lands in one class:

- `libc-qnx8-cfg`: libc 0.2.189+ only knows QNX 8 as target_os = "qnx"; the QNX toolchain says target_os = "nto", target_env = "nto80"
- `missing-libc-binding`: the crate uses a libc item that libc does not define for QNX
- `no-qnx-cfg-arm`: the crate's platform code has no branch for QNX
- `explicitly-unsupported`: the crate stops the build on purpose for this target
- `build-script-cc`: a build script failed compiling C or C++ for QNX
- `build-script-system-lib`: a build script looked for a system library, pkg-config or cmake
- `build-script`: a build script failed
- `rustc-too-old`: needs a newer rustc than the 1.85 QNX ships
- `link`: linking with qcc failed
- `resolve`: cargo could not resolve or download the dependency graph
- `libc-pin-conflict`: the crate requires libc 0.2.189 or newer, so the 0.2.186 pin cannot apply
- `compiler-crash`: the QNX rustc crashed (internal compiler error)
- `no-library`: binary-only package, nothing to link against
- `needs-feature`: the crate needs a feature enabled to build at all; not a QNX problem
- `timeout`: build did not finish inside the time limit
- `other`: other compile error

## Codelab

[Rust on QNX 8.0 from a Windows Host](https://qnx-crate-atlas.vercel.app/codelab/) takes a fresh Windows PC to a Rust binary running on a QNX 8.0 VM: install the QNX Rust toolchain, get a matching cargo, build for both targets, hit and fix the libc trap, boot QNX 8.0 under QEMU in WSL2 and run the binary over SSH. Source: [`codelab/rust-on-qnx-windows/rust-on-qnx-windows.md`](codelab/rust-on-qnx-windows/rust-on-qnx-windows.md) (claat format, the same as qnx.github.io/codelabs). Its environment, build and libc commands are run by [`codelab/verify.bat`](codelab/verify.bat).

## Upstream fixes

- aws-lc-rs [#1292](https://github.com/aws/aws-lc-rs/pull/1292): merged Nov 11 2026. Builds aws-lc-sys for QNX 8.0, which unblocks rustls, tokio-rustls, hyper-rustls and reqwest. Reaches users in the next aws-lc-sys release.
- aws-lc [#3780](https://github.com/aws/aws-lc/pull/3780): open. Adds QNX aarch64 CPU feature detection (reads the system page cpuinfo flags), which fixes the aarch64 rustls link error `undefined reference to OPENSSL_cpuid_setup`. Linked for aarch64 QNX; runtime on aarch64 QNX not executed yet. Reaches aws-lc-rs through its aws-lc submodule.
- target-lexicon [#138](https://github.com/bytecodealliance/target-lexicon/pull/138): QNX triples, including upstream's `x86_64-pc-qnx` / `aarch64-unknown-qnx` (the parse failure that blocks pyo3 there). Open, in review.
- rustix [#1750](https://github.com/bytecodealliance/rustix/pull/1750): open. Builds rustix for upstream Rust's QNX 8.0 targets (`target_os = "qnx"`) through an `nto` cfg, and limits `AF_NS` to io-pkt so `all-apis` builds on the io-sock targets.
- memmap2 [#182](https://github.com/RazrFalcon/memmap2-rs/pull/182): open. QNX has no `madvise()`; uses `posix_madvise()` so memmap2 (and addr2line) build for every QNX target, the SDP 8.0 ones included.

## Found along the way

- `mkqnximage` 0.5.0 builds images whose `sshd` dies at boot with OpenSSH package 0.5.1 (`/usr/libexec/sshd-auth does not exist`). The workaround is in the codelab.
- On Windows, `cargo add libc` writes `libc = "0.2.191"`, which makes the 0.2.186 pin impossible. Use `cargo add libc@0.2`.
- One QNX `rustc` crash (internal compiler error in the parser) appeared once on `serde_core` and did not reproduce on retry.

## License

MIT. The codelab web components in `site/codelab/js` are Google's Codelabs components (Apache 2.0). QNX is a trademark of BlackBerry Limited; this project is not affiliated with or endorsed by BlackBerry or QNX.

Built by Matthew Busel for the QNX Global Hackathon 2026.
