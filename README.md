# SnipStudio — Windows screenshot and screen recorder (Rust)

SnipStudio is a dark-mode Rust/egui desktop app. **Screenshot** and **Screen recording** now use **one identical full-desktop rectangle picker**, instead of selecting from a scaled-down preview.

## Use it

1. Select **▣ Screenshot** or **● Screen recording** in the top bar.
2. Click **＋ New selection**. SnipStudio hides its main window and displays a dimmed, frozen image of your desktop over all connected displays.
3. **Click and drag** to select the rectangle. The area inside the blue border stays bright. Release the mouse to confirm. Press **Esc** or **right-click** to cancel.
4. In **Screenshot** mode, the selected image appears in the preview. Use **Save image** (PNG/JPEG/BMP) or **Copy image**. Hold **Shift** while dragging inside the selected preview area to draw annotations.
5. In **Screen recording** mode, recording begins when you finish the drag. The chosen rectangle is recorded to MP4. Return to SnipStudio and click **■ Stop recording** to finish and save.

Both modes run the same `selection::select_area` native Windows selection overlay in `src/selection.rs`. The main app stays dark.

## Build a standalone Windows EXE

1. Install **Rust** (stable toolchain) from https://rustup.rs/.
2. Install **Visual Studio Build Tools** with the **Desktop development with C++** workload (MSVC compiler/linker).
3. Extract this ZIP completely.
4. Double-click **`Build SnipStudio.bat`**.
5. When it succeeds, run **`publish\SnipStudio.exe`**.

The build script downloads FFmpeg into `assets\ffmpeg.exe`, compresses/embeds it in the exe at compile time, and copies the finished app to `publish`. At first recording, the app extracts FFmpeg to `%LOCALAPPDATA%\SnipStudio\Runtime`. You should not have to separately install FFmpeg when using the build script.

For a quick source-check without an FFmpeg download, run `cargo check --target x86_64-pc-windows-msvc` in the extracted folder after installing the toolchain. In that case, recording needs `ffmpeg.exe` in PATH if you run the app without bundling it.

## How shared selection works

- SnipStudio briefly hides the main app, takes a fresh snapshot of **all enumerated monitors**, and combines their real pixel coordinates into a single virtual-desktop image. This includes screens to the left or above the primary monitor (negative virtual-desktop origins).
- A native Windows topmost popup fills the combined desktop. It draws a dimmed snapshot, a bright selected area, a blue rectangle, and an area-size label while the mouse is dragged.
- The selected rectangle is returned in *actual screen pixels*, so PNG crop coordinates and FFmpeg recording offsets refer to the same region.
- Only after the overlay closes does the app save or record. Recording dimensions are rounded **down** to even numbers where needed for H.264 instead of extending outside the chosen rectangle.

## Existing features preserved

- Dark egui interface.
- PNG/JPEG/BMP screenshot saving and clipboard copy.
- Pen/highlighter annotations with **Shift + drag** and an eraser for nearby strokes.
- Screen recording with FFmpeg `gdigrab`, H.264 MP4, and optional microphone through an exact DirectShow device name.
- FFmpeg gets up to 15 seconds to finalize the MP4. If save is canceled or fails, the recording is preserved under `%LOCALAPPDATA%\SnipStudio\PendingRecordings`.
- Size-focused Rust release profile and embedded compressed FFmpeg.

## Known limitations / testing notes

- The desktop selection overlay is **Windows-only**. The capture/overlay implementation is new and needs a test on a real Windows PC with your monitor layout and display scaling.
- The app window becomes visible again when recording starts. Move it outside the selected rectangle to avoid capturing SnipStudio itself. Recording controls remain in the main window; there is not yet a floating Stop toolbar or global stop hotkey.
- The desktop is presented as a *frozen, dimmed screenshot* during selection, not as a live transparent glass overlay. This avoids capturing the app's own selection rectangle in the recording.
- Microphone capture requires the exact DirectShow input device name; **system audio** is not yet implemented.
- FFmpeg must support `gdigrab`, `libx264` and (for microphone) DirectShow/AAC.
- An image memory safeguard limits the combined virtual desktop to 80 million pixels.
- **The project has not been built on Windows in this environment**; the Rust and MSVC toolchains are unavailable here. Compilation and actual recording/monitor/DPI behavior must be verified locally.

## Licensing

Review FFmpeg redistribution requirements and retain any required notices for the exact FFmpeg build you package (GPL/LGPL may apply).
