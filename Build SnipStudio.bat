@echo off
setlocal
cd /d "%~dp0"
echo SnipStudio Rust build
where cargo >nul 2>nul || (echo Rust is not installed. Install the stable Rust toolchain from https://rustup.rs/ & pause & exit /b 1)
if not exist assets\ffmpeg.exe (
  echo Downloading FFmpeg for the standalone recorder...
  powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile 'assets\ffmpeg.zip'; Expand-Archive -Force 'assets\ffmpeg.zip' 'assets\ffmpeg_unpack'; $f=Get-ChildItem 'assets\ffmpeg_unpack' -Recurse -Filter ffmpeg.exe | Select-Object -First 1; if(-not $f){throw 'ffmpeg.exe not found in downloaded archive'}; Copy-Item $f.FullName 'assets\ffmpeg.exe' -Force; Remove-Item 'assets\ffmpeg.zip','assets\ffmpeg_unpack' -Recurse -Force"
  if errorlevel 1 (echo FFmpeg download failed. Build can still use ffmpeg.exe from PATH, or retry later. & pause & exit /b 1)
)
echo Installing Windows target if needed...
rustup target add x86_64-pc-windows-msvc
echo Building optimized Windows executable...
cargo build --release --target x86_64-pc-windows-msvc
if errorlevel 1 (echo Build failed. Ensure the MSVC C++ Build Tools are installed. & pause & exit /b 1)
if not exist publish mkdir publish
copy /y target\x86_64-pc-windows-msvc\release\snipstudio.exe publish\SnipStudio.exe >nul
echo Done: publish\SnipStudio.exe
pause
