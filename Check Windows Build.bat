@echo off
setlocal
cd /d "%~dp0"
echo Checking SnipStudio Rust source for Windows...
where cargo >nul 2>nul || (echo Rust/Cargo is missing. Install it from https://rustup.rs/ & pause & exit /b 1)
rustup target add x86_64-pc-windows-msvc
if errorlevel 1 (echo Could not install Rust's Windows target. & pause & exit /b 1)
cargo check --target x86_64-pc-windows-msvc
if errorlevel 1 (echo Windows source check FAILED. See compiler messages above. & pause & exit /b 1)
echo Windows Rust source check passed.
pause
