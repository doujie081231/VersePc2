@echo off
call "F:\vs\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
set RUSTUP_HOME=F:\versepc2\.tools\rustup
set CARGO_HOME=F:\versepc2\.tools\cargo
set PATH=F:\tools\node\node-v20.19.6-win-x64;F:\versepc2\.tools\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin;F:\versepc2\.tools\cargo\bin;%PATH%
cargo check -p verse-tauri --all-targets --manifest-path F:\versepc2\src-tauri\Cargo.toml 2>&1