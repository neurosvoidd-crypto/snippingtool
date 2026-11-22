# SnipStudio — Rust rewrite (initial port)

This is a new Rust/egui implementation based on the uploaded .NET 8 WinForms source. It is not a line-for-line translation.

## Build on Windows

1. Install the stable Rust toolchain with rustup: https://rustup.rs/
2. Install Visual Studio Build Tools with the **Desktop development with C++** workload (MSVC target).
3. Double-click `Build SnipStudio.bat`.
4. The executable is copied to `publish\SnipStudio.exe`.

The batch file adds the Windows MSVC Rust target, downloads FFmpeg, embeds a compressed copy in the executable, and the app extracts it under `%LOCALAPPDATA%\SnipStudio\Runtime` when recording is first used. If you build directly with Cargo without the FFmpeg asset, recording requires `ffmpeg.exe` in PATH.

## Implemented in this initial Rust port

- Dark desktop interface using egui.
- Capture of the first enumerated display and rectangular selection in the app's preview.
- PNG/JPEG/BMP saving and clipboard copy.
- Pen and highlighter annotations by holding **Shift** and dragging inside the selected area.
- Eraser removes nearby annotation strokes; Clear edits removes all annotations.
- MP4 recording of the selected rectangle using FFmpeg `gdigrab`.
- Optional microphone input using FFmpeg DirectShow device names.
- Stop-to-finalize behavior, Save dialog, and preservation of the MP4 under `%LOCALAPPDATA%\SnipStudio\PendingRecordings` if saving is cancelled or fails.
- Compressed embedded FFmpeg payload and size-focused release profile.

## Latest fixes in this package

- Explicit `f32` suffixes on egui stroke widths avoid the `float_literal_f32_fallback` future-incompatibility warnings from newer Rust compilers.
- `rect_stroke` calls use the three-argument API supported by egui 0.29.
- Screenshot pixels are copied into the app's `image` 0.25 buffer to avoid the `screenshots`/`image` 0.24 type mismatch.
- Recording stop now gives FFmpeg up to 15 seconds to finalize the MP4, checks its exit status, and reports the preserved temporary-file path on failure instead of treating a partial file as a successful recording.

## Differences and limitations

- This is an initial port, not yet a verified feature-complete replacement. Region selection is made on a captured-screen preview inside the app, rather than a transparent overlay directly over the desktop.
- Only the first enumerated display is captured. Multi-monitor selection and negative-origin virtual-desktop capture are not implemented.
- Microphone input depends on an exact DirectShow device name. System audio capture is not implemented.
- FFmpeg recording depends on a compatible FFmpeg build with `gdigrab`, `libx264`, and (for microphone recording) `dshow`/AAC support.
- The source has not been compiled or run in this environment because Rust/Cargo and a Windows toolchain are not installed here. A Windows build and end-to-end recording test are required before distributing the EXE.

## Licensing

FFmpeg redistribution terms depend on the exact build downloaded. Keep its license notices with distributed builds and review the applicable GPL/LGPL obligations before redistribution.
