use flate2::{write::GzEncoder, Compression};
use std::{env, fs, io::{self, Write}, path::PathBuf};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=assets/ffmpeg.exe");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let source = PathBuf::from("assets/ffmpeg.exe");
    let target = out.join("ffmpeg.exe.gz");
    let input = fs::read(&source).unwrap_or_default();
    let file = fs::File::create(target)?;
    let mut encoder = GzEncoder::new(file, Compression::best());
    encoder.write_all(&input)?;
    encoder.finish()?;
    if input.is_empty() {
        println!("cargo:warning=No bundled FFmpeg found; recording will require ffmpeg.exe in PATH. Run Build SnipStudio.bat to bundle it.");
    }
    Ok(())
}
