@echo off
REM Runs the Windows-side commands of the codelab "Rust on QNX 8.0 from a Windows host" in a fresh
REM folder, so the tutorial is tested, not just written. Usage: codelab\verify.bat [SDP path]
setlocal
set SDP=%~1
if "%SDP%"=="" set SDP=%USERPROFILE%\qnx\qnx800
set WORK=%TEMP%\rust-qnx-codelab
if exist "%WORK%" rmdir /s /q "%WORK%"
mkdir "%WORK%" && cd /d "%WORK%"
set CARGO_TARGET_DIR=
set CARGO_BUILD_JOBS=2

echo === step: environment
call "%SDP%\qnxsdp-env.bat" >nul
@echo off
set RUSTC=%QNX_HOST%\usr\bin\rustc.exe
rustc -vV | findstr /b "rustc host"
rustc --print cfg --target x86_64-pc-nto-qnx800 | findstr "target_os target_env"
cargo +1.85.0 -V

echo === step: hello
cargo +1.85.0 new hello-qnx --vcs none >nul 2>&1
cd hello-qnx
> src\main.rs (
echo fn main^(^) {
echo     println!^("hello from Rust on QNX ^({} / {}^)", std::env::consts::OS, std::env::consts::ARCH^);
echo     let t = std::thread::spawn^(^|^| 6 * 7^);
echo     println!^("thread says {}", t.join^(^).unwrap^(^)^);
echo }
)
cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800 -q || goto :fail
cargo +1.85.0 build --release --target aarch64-unknown-nto-qnx800 -q || goto :fail
dir /b target\x86_64-pc-nto-qnx800\release\hello-qnx target\aarch64-unknown-nto-qnx800\release\hello-qnx
"%QNX_HOST%\usr\bin\ntox86_64-readelf.exe" -h target\x86_64-pc-nto-qnx800\release\hello-qnx | findstr "OS/ABI Machine"

echo === step: libc trap (this build is expected to FAIL)
cargo +1.85.0 add libc@0.2 -q
findstr /c:"name = \"libc\"" /c:"version = \"0.2" Cargo.lock
cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800 -q 2>&1 | findstr /c:"error[E" | findstr /n "." | findstr /b "1: 2: 3:"

echo === step: pin libc 0.2.186
cargo +1.85.0 update -p libc --precise 0.2.186 -q
cargo +1.85.0 build --release --target x86_64-pc-nto-qnx800 -q && echo PINNED_BUILD_OK || goto :fail
cargo +1.85.0 build --release --target aarch64-unknown-nto-qnx800 -q && echo PINNED_BUILD_AARCH64_OK || goto :fail
echo === codelab commands verified
exit /b 0
:fail
echo === FAILED
exit /b 1
