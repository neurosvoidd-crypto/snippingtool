# Windows smoke-test checklist

1. Run `Check Windows Build.bat` and confirm Cargo reports a successful `cargo check` without Rust errors.
2. Run `Build SnipStudio.bat`; check `publish\SnipStudio.exe` exists, opens normally, and keeps its dark UI.
3. Pick **Screenshot** > **New selection**. Confirm the app hides, the dimmed desktop overlay appears, and dragging shows a bright rectangle with blue outline and pixel dimensions.
4. Release to capture. Verify **Save image**, **Copy image**, and **Shift + drag** annotation in the preview.
5. Retry and press **Esc**; the overlay must close without a screenshot. Also test right-click cancellation.
6. Pick **Screen recording** > **New selection**. Confirm you see the **same overlay**. Release over a moving window, wait a few seconds, return to SnipStudio, and click **Stop recording**. Save the MP4; verify playback and the recorded crop.
7. Try a second monitor positioned **left/above** the primary display. Verify the selected rectangle uses the correct negative/positive desktop coordinates in both modes.
8. Try Windows display scaling at 125% or 150%. Verify the selection rectangle aligns with what is actually saved/recorded.
9. If using a microphone, check the exact DirectShow device name and confirm audio. Test with microphone unchecked as well.
10. Cancel the MP4 save dialog and check that the temporary recording remains in `%LOCALAPPDATA%\SnipStudio\PendingRecordings`.

For initial testing, keep the SnipStudio window outside the selected recording rectangle: the main window reappears once recording starts. System-audio recording and a floating stop control are not part of this update.
