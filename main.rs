#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arboard::Clipboard;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use screenshots::Screen;
use std::{fs, io::Write, path::PathBuf, process::{Child, Command, Stdio}, thread, time::Duration};

mod selection;

const EMBEDDED_FFMPEG_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ffmpeg.exe.gz"));

#[derive(Clone, Copy, PartialEq)]
enum Tool { Pen, Highlighter, Eraser }

#[derive(Clone, Copy, PartialEq)]
enum Mode { Screenshot, Recording }

struct Recorder { child: Child, temp_path: PathBuf }
struct Annotation { points: Vec<[f32; 2]>, tool: Tool }

struct SnipStudio {
    screen: Option<RgbaImage>,
    screen_origin: (i32, i32),
    selected: Option<[u32; 4]>,
    mode: Mode,
    texture: Option<egui::TextureHandle>,
    tool: Tool,
    strokes: Vec<Annotation>,
    active_stroke: Vec<[f32; 2]>,
    annotating: bool,
    recorder: Option<Recorder>,
    microphone: bool,
    microphone_name: String,
    status: String,
    ffmpeg_path: Option<PathBuf>,
}

impl Default for SnipStudio {
    fn default() -> Self {
        Self { screen: None, screen_origin: (0, 0), selected: None,
            mode: Mode::Screenshot, texture: None, tool: Tool::Pen,
            strokes: Vec::new(), active_stroke: Vec::new(), annotating: false, recorder: None,
            microphone: false, microphone_name: String::new(), status: "Ready".into(), ffmpeg_path: None }
    }
}

impl SnipStudio {
    /// Capture all displays into one image, preserving their virtual-desktop
    /// coordinates (including monitors placed left/above the primary display).
    fn capture_screen(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let screens = Screen::all().map_err(|e| format!("Cannot enumerate monitors: {e}"))?;
        if screens.is_empty() { return Err("No monitor was found".into()); }
        let left = screens.iter().map(|s| s.display_info.x).min().unwrap();
        let top = screens.iter().map(|s| s.display_info.y).min().unwrap();
        let right = screens.iter().map(|s| i64::from(s.display_info.x) + i64::from(s.display_info.width)).max().unwrap();
        let bottom = screens.iter().map(|s| i64::from(s.display_info.y) + i64::from(s.display_info.height)).max().unwrap();
        let width = u32::try_from(right - i64::from(left)).map_err(|e| e.to_string())?;
        let height = u32::try_from(bottom - i64::from(top)).map_err(|e| e.to_string())?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 80_000_000 {
            return Err("The desktop area is too large to capture safely".into());
        }
        let mut combined = RgbaImage::new(width, height);
        for screen in screens {
            let shot = screen.capture().map_err(|e| format!("Monitor capture failed: {e}"))?;
            // screenshots uses a different image crate version; copy raw RGBA pixels.
            let (w, h) = (shot.width(), shot.height());
            let pixels = shot.into_raw();
            let x = u32::try_from(screen.display_info.x - left).map_err(|e| e.to_string())?;
            let y = u32::try_from(screen.display_info.y - top).map_err(|e| e.to_string())?;
            if x + w > width || y + h > height {
                return Err("Monitor layout changed during capture; please try again".into());
            }
            let row_size = w as usize * 4;
            let combined_bytes: &mut [u8] = combined.as_mut();
            for row in 0..h as usize {
                let dst = ((y as usize + row) * width as usize + x as usize) * 4;
                combined_bytes[dst..dst + row_size]
                    .copy_from_slice(&pixels[row * row_size..(row + 1) * row_size]);
            }
        }
        self.screen_origin = (left, top);
        self.screen = Some(combined);
        self.selected = None;
        self.strokes.clear();
        self.active_stroke.clear();
        self.refresh_texture(ctx);
        Ok(())
    }

    /// Both UI modes call this *same* Windows desktop rectangle picker.
    fn begin_selection(&mut self, ctx: &egui::Context) {
        if self.recorder.is_some() {
            self.status = "Stop recording before making another selection".into();
            return;
        }
        // Hide SnipStudio while taking the background snapshot, so the picker
        // shows the real desktop rather than the app's own window.
        let hidden = selection::HiddenApp::new();
        thread::sleep(Duration::from_millis(160));
        if let Err(error) = self.capture_screen(ctx) {
            self.status = error;
            return; // HiddenApp's Drop restores the application.
        }
        let mode_label = match self.mode { Mode::Screenshot => "Screenshot", Mode::Recording => "Recording" };
        let result = selection::select_area(self.screen.as_ref().unwrap(), self.screen_origin, mode_label);
        drop(hidden);
        match result {
            Ok(Some(area)) => {
                self.selected = Some(area);
                self.strokes.clear();
                self.active_stroke.clear();
                match self.mode {
                    Mode::Screenshot => {
                        self.status = format!("Screenshot area: {} × {}. Save or copy your image.", area[2], area[3]);
                    }
                    Mode::Recording => self.start_recording(),
                }
            }
            Ok(None) => {
                self.selected = None;
                self.status = "Selection cancelled (Esc or right-click)".into();
            }
            Err(error) => {
                self.selected = None;
                self.status = format!("Desktop selection failed: {error}");
            }
        }
        ctx.request_repaint();
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        if let Some(img) = &self.screen {
            // Keep preview textures well below typical GPU limits on multi-monitor PCs.
            let longest = img.width().max(img.height());
            let preview = if longest > 4096 {
                let scale = 4096.0_f32 / longest as f32;
                image::imageops::resize(img,
                    (img.width() as f32 * scale).round().max(1.0) as u32,
                    (img.height() as f32 * scale).round().max(1.0) as u32,
                    image::imageops::FilterType::Triangle)
            } else { img.clone() };
            let size = [preview.width() as usize, preview.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, preview.as_raw());
            self.texture = Some(ctx.load_texture("capture-preview", color, egui::TextureOptions::LINEAR));
        }
    }

    fn selected_image(&self) -> Option<RgbaImage> {
        let src = self.screen.as_ref()?;
        let (x, y, w, h) = match self.selected {
            Some([x, y, w, h]) => (x, y, w, h),
            None => (0, 0, src.width(), src.height()),
        };
        if w == 0 || h == 0 || x + w > src.width() || y + h > src.height() { return None; }
        Some(image::imageops::crop_imm(src, x, y, w, h).to_image())
    }

    fn save_image(&mut self) {
        let Some(mut img) = self.selected_image() else { self.status = "Capture an area first".into(); return; };
        self.apply_strokes(&mut img);
        let Some(path) = rfd::FileDialog::new().set_file_name("SnipStudio.png")
            .add_filter("PNG image", &["png"]).add_filter("JPEG image", &["jpg", "jpeg"])
            .add_filter("Bitmap", &["bmp"]).save_file() else { self.status = "Save cancelled".into(); return; };
        let format = match path.extension().and_then(|x| x.to_str()).unwrap_or("png").to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => ImageFormat::Jpeg, "bmp" => ImageFormat::Bmp, _ => ImageFormat::Png,
        };
        let result = if format == ImageFormat::Jpeg {
            DynamicImage::ImageRgba8(img).to_rgb8().save_with_format(&path, format)
        } else {
            DynamicImage::ImageRgba8(img).save_with_format(&path, format)
        };
        self.status = match result { Ok(()) => format!("Saved {}", path.display()), Err(e) => format!("Image save failed: {e}") };
    }

    fn copy_image(&mut self) {
        let Some(mut img) = self.selected_image() else { self.status = "Capture an area first".into(); return; };
        self.apply_strokes(&mut img);
        match Clipboard::new().and_then(|mut c| c.set_image(arboard::ImageData {
            width: img.width() as usize, height: img.height() as usize, bytes: img.into_raw().into(),
        })) {
            Ok(()) => self.status = "Copied image to clipboard".into(),
            Err(e) => self.status = format!("Clipboard copy failed: {e}"),
        }
    }

    fn apply_strokes(&self, img: &mut RgbaImage) {
        for stroke in &self.strokes {
            if stroke.tool == Tool::Eraser { continue; }
            for pair in stroke.points.windows(2) {
                let color = match stroke.tool { Tool::Highlighter => Rgba([255, 220, 0, 150]), Tool::Eraser => Rgba([0,0,0,0]), Tool::Pen => Rgba([0, 120, 212, 255]) };
                let (x0, y0) = (pair[0][0] * img.width() as f32, pair[0][1] * img.height() as f32);
                let (x1, y1) = (pair[1][0] * img.width() as f32, pair[1][1] * img.height() as f32);
                draw_line(img, x0 as i32, y0 as i32, x1 as i32, y1 as i32, color);
            }
        }
    }

    fn ensure_ffmpeg(&mut self) -> Result<PathBuf, String> {
        if let Some(p) = &self.ffmpeg_path { if p.exists() { return Ok(p.clone()); } }
        if !EMBEDDED_FFMPEG_GZ.is_empty() {
            let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
                .join("SnipStudio").join("Runtime");
            fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let exe = root.join("ffmpeg.exe");
            if !exe.exists() {
                let mut decoder = flate2::read::GzDecoder::new(EMBEDDED_FFMPEG_GZ);
                let mut file = fs::File::create(&exe).map_err(|e| e.to_string())?;
                std::io::copy(&mut decoder, &mut file).map_err(|e| e.to_string())?;
            }
            self.ffmpeg_path = Some(exe.clone());
            return Ok(exe);
        }
        self.ffmpeg_path = Some(PathBuf::from("ffmpeg.exe"));
        Ok(PathBuf::from("ffmpeg.exe"))
    }

    fn start_recording(&mut self) {
        let Some([x, y, w, h]) = self.selected else { self.status = "Select a recording area first".into(); return; };
        if w < 2 || h < 2 { self.status = "Selected area is too small".into(); return; }
        // H.264/YUV420 requires even dimensions. Crop inward, never beyond the selection.
        let (video_w, video_h) = (even_floor(w), even_floor(h));
        if self.microphone && self.microphone_name.trim().is_empty() { self.status = "Enter the exact DirectShow microphone device name".into(); return; }
        let ffmpeg = match self.ensure_ffmpeg() { Ok(p) => p, Err(e) => { self.status = format!("FFmpeg setup failed: {e}"); return; } };
        let temp_dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
            .join("SnipStudio").join("PendingRecordings");
        if let Err(e) = fs::create_dir_all(&temp_dir) { self.status = format!("Cannot create recording folder: {e}"); return; }
        let stamp = chrono_free_stamp();
        let output = temp_dir.join(format!("Recording-{stamp}.mp4"));
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "warning", "-y", "-f", "gdigrab", "-framerate", "30",
            "-offset_x", &(x as i32 + self.screen_origin.0).to_string(), "-offset_y", &(y as i32 + self.screen_origin.1).to_string(),
            "-video_size", &format!("{}x{}", video_w, video_h), "-i", "desktop"]);
        if self.microphone {
            cmd.args(["-f", "dshow", "-i", &format!("audio={}", self.microphone_name.trim())]);
            cmd.args(["-map", "0:v:0", "-map", "1:a:0"]);
        }
        cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]);
        if self.microphone { cmd.args(["-c:a", "aac", "-b:a", "128k"]); }
        cmd.arg(&output).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(stderr) = child.stderr.take() { thread::spawn(move || { let mut sink = std::io::sink(); let mut r = stderr; let _ = std::io::copy(&mut r, &mut sink); }); }
                self.recorder = Some(Recorder { child, temp_path: output });
                self.status = "Recording… press Stop to finish and save".into();
            }
            Err(e) => self.status = format!("Could not start FFmpeg: {e}. Run the build script to bundle FFmpeg."),
        }
    }

    fn stop_recording(&mut self) {
        let Some(mut rec) = self.recorder.take() else { return; };
        if let Some(mut input) = rec.child.stdin.take() {
            let _ = input.write_all(b"q\n");
        }

        // Give FFmpeg time to write the MP4 index and close the container cleanly.
        let mut exit_status = None;
        for _ in 0..150 {
            match rec.child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(100)),
                Err(_) => break,
            }
        }

        let Some(status) = exit_status else {
            let _ = rec.child.kill();
            let _ = rec.child.wait();
            self.status = format!(
                "FFmpeg did not finish finalizing within 15 seconds. The recording was kept at {}",
                rec.temp_path.display()
            );
            return;
        };
        if !status.success() {
            self.status = format!(
                "FFmpeg exited with status {:?}. The recording was kept at {}",
                status.code(), rec.temp_path.display()
            );
            return;
        }
        if !rec.temp_path.exists()
            || fs::metadata(&rec.temp_path).map(|m| m.len() < 1024).unwrap_or(true)
        {
            self.status = format!(
                "Recording did not produce a valid MP4. Check the selected display and FFmpeg. Temporary file: {}",
                rec.temp_path.display()
            );
            return;
        }
        let Some(dest) = rfd::FileDialog::new().set_file_name("SnipStudio-Recording.mp4")
            .add_filter("MP4 video", &["mp4"]).save_file() else {
                self.status = format!("Save cancelled. Recording kept at {}", rec.temp_path.display()); return;
            };
        match fs::copy(&rec.temp_path, &dest) {
            Ok(_) => { let _ = fs::remove_file(&rec.temp_path); self.status = format!("Recording saved to {}", dest.display()); }
            Err(e) => self.status = format!("Cannot save file: {e}. Recording preserved at {}", rec.temp_path.display()),
        }
    }
}

impl eframe::App for SnipStudio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("SnipStudio");
                ui.separator();
                ui.add_enabled_ui(self.recorder.is_none(), |ui| {
                    ui.selectable_value(&mut self.mode, Mode::Screenshot, "▣ Screenshot");
                    ui.selectable_value(&mut self.mode, Mode::Recording, "● Screen recording");
                });
                ui.separator();
                if self.recorder.is_some() {
                    if ui.button("■ Stop recording").clicked() { self.stop_recording(); }
                } else if ui.button("＋ New selection").clicked() {
                    self.begin_selection(ctx);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if self.mode == Mode::Screenshot {
                    ui.add_enabled_ui(self.selected.is_some(), |ui| {
                        if ui.button("Save image").clicked() { self.save_image(); }
                        if ui.button("Copy image").clicked() { self.copy_image(); }
                        ui.separator();
                        egui::ComboBox::from_label("Annotate")
                            .selected_text(match self.tool { Tool::Pen => "Pen", Tool::Highlighter => "Highlighter", Tool::Eraser => "Eraser" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.tool, Tool::Pen, "Pen");
                                ui.selectable_value(&mut self.tool, Tool::Highlighter, "Highlighter");
                                ui.selectable_value(&mut self.tool, Tool::Eraser, "Eraser");
                            });
                        if ui.button("Clear edits").clicked() {
                            self.strokes.clear();
                            self.active_stroke.clear();
                        }
                    });
                } else {
                    ui.checkbox(&mut self.microphone, "Record microphone");
                    if self.microphone {
                        ui.add(egui::TextEdit::singleline(&mut self.microphone_name)
                            .hint_text("Exact DirectShow microphone name").desired_width(250.0));
                    }
                    if self.recorder.is_some() { ui.strong("● Recording in progress"); }
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(match self.mode {
                Mode::Screenshot => "Screenshot mode: New selection opens the desktop-wide rectangle tool. Drag a region, then Save or Copy. Hold Shift and drag inside the preview selection to annotate.",
                Mode::Recording => "Recording mode: New selection opens the SAME desktop-wide rectangle tool. Drag a region to begin recording it; return here to press Stop.",
            });
            if let (Some(screen), Some(texture)) = (&self.screen, &self.texture) {
                let (screen_w, screen_h) = (screen.width(), screen.height());
                let texture_id = texture.id();
                let avail = ui.available_size();
                let image_size = Vec2::new(screen_w as f32, screen_h as f32);
                let scale = (avail.x / image_size.x)
                    .min((avail.y - 20.0).max(1.0) / image_size.y).min(1.0);
                let display_size = image_size * scale;
                let (rect, response) = ui.allocate_exact_size(display_size, Sense::click_and_drag());
                ui.painter().image(texture_id, rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if response.drag_started() {
                    self.annotating = self.mode == Mode::Screenshot
                        && self.selected.is_some() && ctx.input(|i| i.modifiers.shift);
                    self.active_stroke.clear();
                }
                if response.dragged() && self.annotating {
                    if let (Some(pos), Some([sx, sy, sw, sh])) = (response.interact_pointer_pos(), self.selected) {
                        let selected_rect = Rect::from_min_size(
                            rect.min + Vec2::new(sx as f32, sy as f32) * scale,
                            Vec2::new(sw as f32, sh as f32) * scale);
                        if selected_rect.contains(pos) {
                            self.active_stroke.push([
                                ((pos.x - selected_rect.min.x) / selected_rect.width()).clamp(0.0, 1.0),
                                ((pos.y - selected_rect.min.y) / selected_rect.height()).clamp(0.0, 1.0),
                            ]);
                        }
                    }
                }
                if response.drag_stopped() {
                    if self.annotating {
                        if self.tool == Tool::Eraser {
                            let points = self.active_stroke.clone();
                            self.strokes.retain(|stroke| !stroke.points.iter().any(|p|
                                points.iter().any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) < 0.035)));
                        } else if self.active_stroke.len() > 1 {
                            self.strokes.push(Annotation {
                                points: self.active_stroke.clone(), tool: self.tool,
                            });
                        }
                    }
                    self.active_stroke.clear();
                    self.annotating = false;
                }
                if let Some([x, y, w, h]) = self.selected {
                    let r = Rect::from_min_size(
                        rect.min + Vec2::new(x as f32, y as f32) * scale,
                        Vec2::new(w as f32, h as f32) * scale);
                    ui.painter().rect_stroke(r, 0.0, Stroke::new(2.0_f32, Color32::LIGHT_BLUE));
                    for stroke in &self.strokes {
                        let color = match stroke.tool {
                            Tool::Pen => Color32::from_rgb(0, 120, 212),
                            Tool::Highlighter => Color32::from_rgba_unmultiplied(255, 220, 0, 130),
                            Tool::Eraser => Color32::TRANSPARENT,
                        };
                        let pts: Vec<Pos2> = stroke.points.iter().map(|p|
                            Pos2::new(r.min.x + p[0] * r.width(), r.min.y + p[1] * r.height())).collect();
                        if pts.len() > 1 {
                            ui.painter().add(egui::Shape::line(pts, Stroke::new(
                                if stroke.tool == Tool::Highlighter { 9.0_f32 } else { 2.5_f32 }, color)));
                        }
                    }
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(90.0);
                    ui.heading("Ready to capture");
                    ui.label("Choose Screenshot or Screen recording and click New selection.");
                });
            }
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| { ui.label(&self.status); });
        let ended = self.recorder.as_mut().and_then(|r|
            r.child.try_wait().ok().flatten().map(|status| (status, r.temp_path.clone())));
        if let Some((status, path)) = ended {
            self.recorder.take();
            let valid = fs::metadata(&path).map(|m| m.len() > 1024).unwrap_or(false);
            self.status = if valid {
                format!("Recorder exited ({}). File preserved at {}", status, path.display())
            } else {
                format!("Recorder stopped unexpectedly ({}). Check FFmpeg and capture permissions.", status)
            };
        }
        if self.recorder.is_some() { ctx.request_repaint_after(Duration::from_millis(250)); }
    }
}

fn draw_line(img: &mut RgbaImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgba<u8>) {
    let dx = (x1 - x0).abs(); let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs(); let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy; let (mut x, mut y) = (x0, y0);
    loop { if x >= 0 && y >= 0 && x < img.width() as i32 && y < img.height() as i32 { img.put_pixel(x as u32, y as u32, color); }
        if x == x1 && y == y1 { break; } let e2 = 2 * err; if e2 >= dy { err += dy; x += sx; } if e2 <= dx { err += dx; y += sy; } }
}
fn even_floor(n: u32) -> u32 { n & !1 }
fn chrono_free_stamp() -> String { use std::time::{SystemTime, UNIX_EPOCH}; SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string() }

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([960.0, 680.0]).with_min_inner_size([680.0, 480.0]), ..Default::default() };
    eframe::run_native("SnipStudio", options, Box::new(|cc| {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.window_fill = Color32::from_rgb(28, 28, 30);
        style.visuals.panel_fill = Color32::from_rgb(24, 24, 26);
        cc.egui_ctx.set_style(style);
        Ok(Box::new(SnipStudio::default()))
    }))
}
