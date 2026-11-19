# Changelog

## 0.2.0 (2026-11-19)

### Why

A portability claim on crates.io costs nothing to make and nothing checks it. `categories = ["no-std"]`, `keywords = ["embedded"]` and "cross-platform" in a README are read by people choosing dependencies for firmware and RTOS products, and some of those claims are false: in the top 500 crates, 8 have a no_std, embedded or portable claim that does not compile on the target it names, and 6 more stop the build on purpose. The person who finds out is the downstream user, after they have committed to the dependency. 0.2.0 makes the atlas a gate that fails at the claim, with the exact error.

### Added

- `atlas claims [PATH] | --crate NAME[@VERSION]`: reads a crate's portability claims (categories, keywords, description, `#![no_std]`, README, crate docs, each with file and line), builds each claim for a target that tests it (no_std / embedded on `thumbv7em-none-eabihf`; QNX / portable on upstream `x86_64-pc-qnx` through nightly `-Zbuild-std`, or the QNX SDP toolchain with `--qnx sdp`), and **exits 1** when a claim does not build. Output: text, `--format json`, or `--format github` (an error annotation on the claim's line plus a step summary). `--strict` also fails on checks that cannot be decided on the host; `--also bare-metal,qnx` runs a check without a claim.
- The first error names the crate that broke (this crate or a dependency) with file:line, and for a dependency the `cargo tree -i -e features` path that turned `std` on.
- `atlas claims-atlas`: the same check over the top 500 (`data/claims.jsonl`, `site/data/claims.json`), with `--qnx-all` to build every crate for upstream QNX, claim or not.
- `action.yml`: `uses: Mattbusel/qnx-crate-atlas@v0.2.0` in a workflow.
- `examples/claims`: an honest and a broken no_std crate; CI asserts the first passes and the second fails with the right error.
- The site leads with the claims atlas.

### Changed

- `FirstError` records the line of the primary span (`line`, omitted when unknown); existing JSON still loads.
- The crate's own metadata: the `embedded` keyword and category are gone. This is a host tool and does not run on a microcontroller; by its own rule it should not say so.
- `rustfmt.toml` added and the code formatted with it; clippy is clean and CI now enforces both.
- The QNX SDP results now cover the refreshed top 500 (jiff and ed25519-dalek entered the list; both build for both QNX targets).

### Not changed

- `run`, `compat`, `ontarget`, `report` and `fix` behave as in 0.1.0.
