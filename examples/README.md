# Examples proven on QNX 8.0

All three build with the QNX SDP 8.0 Rust toolchain (`cargo +1.85.0`, `RUSTC=%QNX_HOST%\usr\bin\rustc.exe`) after `python patches/prepare.py`, and were run on a QNX 8.0 x86_64 VM:

| Example | Needs | Output on QNX 8.0 |
|---|---|---|
| `socket2-keepalive` | libc compatibility copy (`TCP_KEEPIDLE`) | `socket2 keepalive on QNX: true, pid 528405` |
| `tokio-net` | libc compatibility copy | `tokio net on QNX: accepted 127.0.0.1:15585` |
| `rustls-aws-lc` | libc copy + aws-lc-sys fix | `rustls + aws-lc-rs on QNX: 9 cipher suites, first TLS13_AES_256_GCM_SHA384, random byte 60` |

`tokio-net` turns on every tokio feature (`full`, so `net` too), binds a TCP listener and accepts a connection; it builds for both targets and ran on the x86_64 VM. `rustls-aws-lc` links for x86_64 only; aarch64 still needs a QNX variant of aws-lc's aarch64 CPU feature detection (`OPENSSL_cpuid_setup`).
