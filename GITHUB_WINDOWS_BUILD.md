# Build SnipStudio on a Windows MSVC runner

This project includes `.github/workflows/windows-msvc.yml`. It runs on GitHub Actions using a real Windows 2022 runner, Rust `x86_64-pc-windows-msvc`, Visual C++ MSVC tools and the Windows SDK.

## Important

This ZIP contains workflow **configuration**, not a compiled executable. The assistant environment does not include Windows/MSVC or network access to install Rust, so it did not run or verify the Windows build.

## Upload the files to GitHub

Upload **the contents** of the `SnipStudio_Rust` folder into the root of your repository. This includes:

- `Cargo.toml`, `build.rs`, the build scripts, and the README
- `src/main.rs` **and** `src/selection.rs` (the GitHub repo previously had `main.rs` at the root)
- `.github/workflows/windows-msvc.yml`

Using `git` from the cloned repository, after copying the project files into it:

```bash
git add -A
git commit -m "Add shared region selection and Windows MSVC build"
git push origin main
```

## Run and download

1. Open the GitHub repository, then choose **Actions**.
2. Select **Build SnipStudio for Windows (Rust MSVC)**.
3. Click **Run workflow**, selecting `main`, or push the files to trigger it automatically.
4. Click the Windows x64 / MSVC job to see compiler output and diagnose failures.
5. If the run succeeds, download the **SnipStudio-Windows-x64-MSVC** artifact from the run summary. It contains `SnipStudio.exe`.

The workflow fails explicitly if the FFmpeg download, Rust type-check, compilation, or EXE verification fails. Passing the workflow verifies compilation and basic EXE structure, **not** correct interactive screen selection or screen recording on a desktop. Those still need hands-on testing on Windows.
