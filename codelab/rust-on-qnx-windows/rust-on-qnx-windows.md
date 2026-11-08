id: rust-on-qnx-windows
title: Rust on QNX 8.0 from a Windows Host
summary: Cross-compile Rust for QNX 8.0 on Windows with the QNX Rust toolchain, avoid the libc trap, and run the result on a QNX 8.0 VM.
categories: qnx, rust, windows
tags: beginner
difficulty: 2
status: published
authors: Matthew Busel
feedback_link: https://github.com/Mattbusel/qnx-crate-atlas/issues

# Rust on QNX 8.0 from a Windows Host

## Introduction
Duration: 2:00

### Overview
QNX ships a Rust compiler for QNX 8.0 in the QNX Software Center. This codelab takes a Windows 11 PC from nothing to a Rust program that is cross-compiled on Windows for both QNX 8.0 targets and then runs on a real QNX 8.0 kernel in a virtual machine.

Along the way you will hit the two problems that stop most people, and fix them:

* the QNX Rust package contains `rustc` but no `cargo`, and
* the newest `libc` crate no longer compiles with the QNX toolchain, which breaks every crate that depends on it.

### What you will build
A small Rust program that prints the OS and CPU it runs on and starts a thread, built for `x86_64-pc-nto-qnx800` and `aarch64-unknown-nto-qnx800`, then a second version that calls into `libc`.

### What you need
* Windows 11 x86_64 with about 10 GB free disk space.
* A myQNX account with a QNX SDP 8.0 non-commercial license. See the codelab [Get a Non-Commercial QNX License](https://qnx.github.io/codelabs/get-non-commercial-qnx-license/index.html).
* [rustup](https://rustup.rs) for Windows with the Microsoft C++ build tools (the rustup installer offers to install them). Build scripts and procedural macros still run on Windows.
* For the last step only: WSL 2 with Ubuntu 24.04.

The environment, build and libc commands in this codelab are checked by a script, [`codelab/verify.bat`](https://github.com/Mattbusel/qnx-crate-atlas/blob/main/codelab/verify.bat), which runs them in a fresh folder.

---

## Install QNX SDP 8.0 and the QNX Rust toolchain
Duration: 15:00

1. Download **QNX Software Center 2.0.4 for Windows** from the [QNX download page](https://www.qnx.com/download/group.html?programid=29178) and install it. A per-user install is enough; no administrator rights are needed.
2. Start QNX Software Center, sign in with your myQNX account, and choose **Add Installation**. Pick **QNX Software Development Platform 8.0** and install it to `C:\Users\<you>\qnx\qnx800`. The download is several gigabytes.
3. In the new installation choose **Manage Installation**, open the **Available** tab, search for `rust`, and install **QNX Rust toolchain for Windows host** (package id `com.qnx.qnx800.host.win.x86_64.rust`).

Prefer the command line? The Software Center has one:

```console
"C:\Users\<you>\qnx\QNX Software Center\qnxsoftwarecenter_clt.bat" -myqnx.user="<email>" -myqnx.password="<password>" -destination C:\Users\<you>\qnx\qnx800 -installPackage com.qnx.qnx800.host.win.x86_64.rust
```

When it finishes you have a QNX-built `rustc.exe` here:

```console
C:\Users\<you>\qnx\qnx800\host\win64\x86_64\usr\bin\rustc.exe
```

It ships the Rust standard library for exactly three targets: `x86_64-pc-nto-qnx800`, `aarch64-unknown-nto-qnx800`, and the Windows host (needed for build scripts and procedural macros).

---

## Get a matching cargo
Duration: 3:00

The QNX package has `rustc` 1.85.1 but no `cargo`. Install the cargo that matches it, from Rust 1.85.0, with rustup:

```console
rustup toolchain install 1.85.0 --profile minimal
```

You will run `cargo +1.85.0` and point it at the QNX compiler with the `RUSTC` environment variable. Cargo 1.85 and rustc 1.85.1 are the same release line, so their flags and lockfile formats agree.

Negative
: Do not use your everyday stable cargo with the QNX `rustc`. A newer cargo can pass flags the older compiler does not understand, and its lockfiles may need a newer cargo to read.

---

## Set up the environment
Duration: 3:00

Open a **Command Prompt** (cmd.exe, not PowerShell) and run the SDP environment script, then select the QNX compiler:

```console
call C:\Users\<you>\qnx\qnx800\qnxsdp-env.bat
set RUSTC=%QNX_HOST%\usr\bin\rustc.exe
```

`qnxsdp-env.bat` sets `QNX_HOST` and `QNX_TARGET` and puts `qcc`, the QNX compiler driver, on `PATH`. The QNX Rust targets link with `qcc`, so no linker configuration is needed.

Check what you have:

```console
rustc -vV
rustc --print cfg --target x86_64-pc-nto-qnx800 | findstr "target_os target_env"
cargo +1.85.0 -V
```

You should see:

```console
rustc 1.85.1-dev
host: x86_64-pc-windows-msvc
target_env="nto80"
target_os="nto"
cargo 1.85.0 (d73d2caf9 2024-12-31)
```

Remember `target_os="nto"` and `target_env="nto80"`. They are the reason for the trap two steps from now.

---

## Build hello world for QNX
Duration: 5:00

1. Create a project:

    ```console
    cargo +1.85.0 new hello-qnx
    cd hello-qnx
    ```

2. Replace `src\main.rs` with:

    ```rust
    fn main() {
        println!("hello from Rust on QNX ({} / {})", std::env::consts::OS, std::env::consts::ARCH);
        let t = std::thread::spawn(|| 6 * 7);
        println!("thread says {}", t.join().unwrap());
    }
    ```

3. Build for both QNX 8.0 targets:

    ```console
    cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800
    cargo +1.85.0 build --release --target aarch64-unknown-nto-qnx800
    ```

The binaries are `target\x86_64-pc-nto-qnx800\release\hello-qnx` and `target\aarch64-unknown-nto-qnx800\release\hello-qnx`. They are ELF files for QNX, so they will not run on Windows:

```console
ntox86_64-readelf -h target\x86_64-pc-nto-qnx800\release\hello-qnx | findstr "Machine"
  Machine:                           Advanced Micro Devices X86-64
```

Positive
: If you set `CARGO_TARGET_DIR` globally (some setups do), the binaries land under that folder instead of `target\`.

---

## The libc trap
Duration: 5:00

Almost every crate that talks to the operating system depends on `libc`. Add it:

```console
cargo +1.85.0 add libc@0.2
cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800
```

The build fails inside `libc` itself, with dozens of errors like:

```console
error[E0412]: cannot find type `in_addr` in this scope
   --> ...\libc-0.2.191\src\unix\nto\mod.rs:129:28
error: non-item macro in item position: panic
    --> ...\libc-0.2.191\src\unix\nto\mod.rs:3096:9
     |
3096 |         panic!("Unsupported arch");
```

### Why
Upstream Rust renamed the QNX 8.0 targets to `x86_64-pc-qnx` and `aarch64-unknown-qnx`, which report `target_os = "qnx"`. `libc` 0.2.189 and newer follow the new name and only compile their QNX 8.0 code when `target_os = "qnx"`. The compiler QNX ships still reports `target_os = "nto"`, `target_env = "nto80"`, so none of that code is selected.

### Fix: pin libc 0.2.186
`libc` 0.2.186 is the last release that still recognises `target_env = "nto80"`:

```console
cargo +1.85.0 update -p libc --precise 0.2.186
cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800
cargo +1.85.0 build --release --target aarch64-unknown-nto-qnx800
```

Both builds now succeed.

Negative
: `cargo add libc` (without `@0.2`) writes `libc = "0.2.191"` into `Cargo.toml`, and then the pin is refused with "failed to select a version for the requirement". Keep the requirement at `"0.2"` so cargo is allowed to choose 0.2.186.

### When the pin is not enough
`libc` 0.2.186 predates some QNX 8.0 definitions that later releases added. The most visible gap is the TCP keepalive constants: `socket2`, and with it `tokio` networking, `hyper` clients and `reqwest`, fail with:

```console
error[E0432]: unresolved import `libc::TCP_KEEPIDLE`
```

The [QNX Crate Atlas](https://github.com/Mattbusel/qnx-crate-atlas) ships a script that makes a QNX 8.0 compatible copy of current `libc` by mapping the old spelling onto the new one. Use it with a `[patch]` section:

```console
python patches\make_libc_compat.py %CARGO_HOME%\registry\src\<index>\libc-0.2.191 C:\src\libc-qnx8
```

```toml
[patch.crates-io]
libc = { path = "C:/src/libc-qnx8" }
```

With that patch, `socket2` builds for both targets, and a program that turns on TCP keepalive runs correctly on QNX 8.0.

---

## Run it on QNX 8.0 under QEMU
Duration: 20:00

You need a QNX 8.0 target. The quickest one on a Windows PC is an x86_64 QEMU virtual machine inside WSL 2, which runs with KVM acceleration. The official codelab [Run the QSTI on QEMU using WSL](https://qnx.github.io/codelabs/qsti-qemu-on-wsl/index.html) covers the Quick Start Target Image; the steps below use `mkqnximage` from the SDP instead, which lets you add your SSH key.

1. In PowerShell, add Ubuntu 24.04 and open a root shell in it:

    ```console
    wsl --install -d Ubuntu-24.04
    wsl -d Ubuntu-24.04 -u root
    ```

2. Inside Ubuntu, install QEMU and the networking tools:

    ```console
    apt update && apt install -y qemu-system-x86 qemu-utils bridge-utils net-tools iproute2 dnsmasq-base
    ```

3. Install the Linux host version of QNX SDP 8.0 to `/root/qnx800` with the Linux QNX Software Center (same account, same license), then:

    ```console
    source /root/qnx800/qnxsdp-env.sh
    ssh-keygen -t ed25519 -N "" -f /root/.ssh/id_ed25519
    mkdir /root/qnx-vm && cd /root/qnx-vm
    mkqnximage --type=qemu --arch=x86_64 --ssh-ident=/root/.ssh/id_ed25519.pub --noprompt
    ```

4. Start the network bridge and a DHCP server. The `net.sh` helper's own `dnsmasq` call clashes with WSL's DNS on port 53, so run DHCP only:

    ```console
    bash $QNX_BASE/host/common/mkqnximage/qemu/net.sh /usr/lib/qemu/qemu-bridge-helper /etc/qemu/bridge.conf nat
    dnsmasq --interface=br0 --bind-interfaces --except-interface=lo --listen-address=172.31.1.1 --port=0 --dhcp-range=172.31.1.2,172.31.1.254
    ```

5. Boot the VM headless and copy your binary over (WSL sees your C: drive under `/mnt/c`):

    ```console
    mkqnximage --run=-h &
    IP=$(grep -Eo '172\.31\.1\.[0-9]+' /var/lib/misc/dnsmasq.leases | head -1)
    scp /mnt/c/Users/<you>/hello-qnx/target/x86_64-pc-nto-qnx800/release/hello-qnx root@$IP:/tmp/
    ssh root@$IP /tmp/hello-qnx
    ```

You should see:

```console
hello from Rust on QNX (nto / x86_64)
thread says 42
```

Negative
: If SSH to the VM is refused, check the boot log in `output/qemu.out` for `/usr/libexec/sshd-auth does not exist`. Some SDP package combinations ship an OpenSSH that needs `sshd-auth` while `mkqnximage` does not put it in the image. Adding `bin/sshd-auth=usr/libexec/sshd-auth` to the image build file and `SshdAuthPath /system/bin/sshd-auth` to the sshd configuration in `$QNX_BASE/host/common/mkqnximage/inputs/system.build`, then rebuilding the image, fixes it.

Positive
: WSL stops background processes when the `wsl` call that started them ends. If you script this from Windows, boot, test and shut down the VM inside one `wsl` call.

---

## Check a crate before you depend on it
Duration: 3:00

The [QNX Crate Atlas](https://github.com/Mattbusel/qnx-crate-atlas) has already built the 500 most downloaded crates for both QNX 8.0 targets with this exact toolchain, recorded the first error for each failure, and run unit tests of the top crates on QNX 8.0. Search it before you add a dependency.

To test any crate yourself, build the Atlas CLI with your normal Rust toolchain and probe it:

```console
git clone https://github.com/Mattbusel/qnx-crate-atlas
cd qnx-crate-atlas
cargo build --release
target\release\atlas doctor
target\release\atlas probe tokio --target aarch64-unknown-nto-qnx800
```

`atlas probe` builds the crate inside a throwaway QNX binary, retries with `libc` pinned when needed, and prints the first error and its class.

---

## Summary
Duration: 1:00

You now have:

* the QNX Rust toolchain on Windows, driven by cargo 1.85.0,
* Rust binaries for `x86_64-pc-nto-qnx800` and `aarch64-unknown-nto-qnx800`,
* the libc fix: `cargo update -p libc --precise 0.2.186`, or the compatibility patch when you need newer QNX 8.0 definitions,
* a QNX 8.0 VM in WSL 2 to run and test what you build.

### Learn more
* [QNX Crate Atlas](https://github.com/Mattbusel/qnx-crate-atlas): which crates build for QNX 8.0, and why the others do not.
* [QNX SDP 8.0 documentation](https://www.qnx.com/developers/docs/8.0/com.qnx.doc.qnxsdp.nav/topic/bookset.html)
* [libc issue about the QNX target rename](https://github.com/rust-lang/libc/issues/5341)
