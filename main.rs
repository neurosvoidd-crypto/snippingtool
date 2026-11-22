#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use arboard::Clipboard;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use screenshots::Screen;
use std::{fs, io::Write, path::PathBuf, process::{Child, Command, Stdio}, thread, time::Duration};

const EMBEDDED_FFMPEG_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ffmpeg.exe.gz"));

#[derive(Clone, Copy, PartialEq)]
enum Tool { Pen, Highlighter, Eraser }

struct Recorder { child: Child, temp_path: PathBuf }
struct Annotation { points: Vec<[f32; 2]>, tool: Tool }

struct SnipStudio {
    screen: Option<RgbaImage>,
    screen_origin: (i32, i32),
    selected: Option<[u32; 4]>,
    drag_start: Option<Pos2>,
    drag_rect: Option<Rect>,
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
            drag_start: None, drag_rect: None, texture: None, tool: Tool::Pen,
            strokes: Vec::new(), active_stroke: Vec::new(), annotating: false, recorder: None,
            microphone: false, microphone_name: String::new(), status: "Ready".into(), ffmpeg_path: None }
    }
}

impl SnipStudio {
    fn capture_screen(&mut self, ctx: &egui::Context) {
        match Screen::all() {
            Ok(screens) => match screens.first() {
                Some(screen) => match screen.capture() {
                    Ok(img) => {
                        self.screen_origin = (screen.display_info.x, screen.display_info.y);
                        // `screenshots` currently uses image 0.24 while this app uses
                        // image 0.25. Transfer the pixel buffer instead of mixing types.
                        let (width, height) = (img.width(), img.height());
                        match RgbaImage::from_raw(width, height, img.into_raw()) {
                            Some(captured) => {
                                self.screen = Some(captured);
                                self.selected = None;
                                self.strokes.clear();
                                self.active_stroke.clear();
                                self.refresh_texture(ctx);
                                self.status = "Drag over the preview to select an area".into();
                            }
                            None => self.status = "Screen capture returned an invalid pixel buffer".into(),
                        }
                    }
                    Err(e) => self.status = format!("Screen capture failed: {e}"),
                },
                None => self.status = "No display was found".into(),
            },
            Err(e) => self.status = format!("Could not enumerate displays: {e}"),
        }
    }

    fn refresh_texture(&mut self, ctx: &egui::Context) {
        if let Some(img) = &self.screen {
            let size = [img.width() as usize, img.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
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
            "-video_size", &format!("{}x{}", even(w), even(h)), "-i", "desktop"]);
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
                if ui.button("▣ Capture region").clicked() { self.capture_screen(ctx); }
                if ui.button("Save image").clicked() { self.save_image(); }
                if ui.button("Copy").clicked() { self.copy_image(); }
                ui.separator();
                egui::ComboBox::from_label("Tool").selected_text(match self.tool { Tool::Pen => "Pen", Tool::Highlighter => "Highlighter", Tool::Eraser => "Eraser" })
                    .show_ui(ui, |ui| { ui.selectable_value(&mut self.tool, Tool::Pen, "Pen"); ui.selectable_value(&mut self.tool, Tool::Highlighter, "Highlighter"); ui.selectable_value(&mut self.tool, Tool::Eraser, "Eraser"); });
                if ui.button("Clear edits").clicked() { self.strokes.clear(); self.active_stroke.clear(); }
                ui.separator();
                if self.recorder.is_some() {
                    if ui.button("■ Stop recording").clicked() { self.stop_recording(); }
                } else if ui.button("● Record").clicked() { self.start_recording(); }
                ui.checkbox(&mut self.microphone, "Microphone");
                if self.microphone { ui.add(egui::TextEdit::singleline(&mut self.microphone_name).hint_text("Exact microphone device name").desired_width(180.0)); }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Select an area: click Capture region, then drag across the screen preview. After selection, hold Shift and drag to annotate. Recording captures that rectangle.");
            if self.screen.is_some() && self.texture.is_some() {
                let (screen_w, screen_h) = self.screen.as_ref().map(|i| (i.width(), i.height())).unwrap_or((1,1));
                let texture_id = self.texture.as_ref().map(|t| t.id()).unwrap();
                let avail = ui.available_size();
                let image_size = Vec2::new(screen_w as f32, screen_h as f32);
                let scale = (avail.x / image_size.x).min((avail.y - 20.0).max(1.0) / image_size.y).min(1.0);
                let display_size = image_size * scale;
                let (rect, response) = ui.allocate_exact_size(display_size, Sense::click_and_drag());
                ui.painter().image(texture_id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                if response.drag_started() {
                    self.drag_start = response.interact_pointer_pos();
                    self.drag_rect = None;
                    self.annotating = self.selected.is_some() && ctx.input(|i| i.modifiers.shift);
                    self.active_stroke.clear();
                }
                if response.dragged() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        if self.annotating {
                            if let Some([sx, sy, sw, sh]) = self.selected {
                                let selected_rect = Rect::from_min_size(rect.min + Vec2::new(sx as f32, sy as f32) * scale, Vec2::new(sw as f32, sh as f32) * scale);
                                if selected_rect.contains(pos) {
                                    self.active_stroke.push([((pos.x-selected_rect.min.x)/selected_rect.width()).clamp(0.0,1.0), ((pos.y-selected_rect.min.y)/selected_rect.height()).clamp(0.0,1.0)]);
                                }
                            }
                        } else if let Some(start) = self.drag_start {
                            self.drag_rect = Some(Rect::from_two_pos(start, pos).intersect(rect));
                        }
                    }
                }
                if response.drag_stopped() {
                    if self.annotating {
                        if self.tool == Tool::Eraser {
                            let points = self.active_stroke.clone();
                            self.strokes.retain(|stroke| !stroke.points.iter().any(|p| points.iter().any(|q| (p[0]-q[0]).hypot(p[1]-q[1]) < 0.035)));
                        } else if self.active_stroke.len() > 1 {
                            self.strokes.push(Annotation { points: self.active_stroke.clone(), tool: self.tool });
                        }
                        self.active_stroke.clear();
                    } else if let Some(r) = self.drag_rect {
                        let x = ((r.min.x - rect.min.x) / scale).round().max(0.0) as u32;
                        let y = ((r.min.y - rect.min.y) / scale).round().max(0.0) as u32;
                        let w = ((r.width()) / scale).round().max(0.0) as u32;
                        let h = ((r.height()) / scale).round().max(0.0) as u32;
                        if w >= 2 && h >= 2 { self.selected = Some([x, y, w.min(screen_w-x), h.min(screen_h-y)]); self.strokes.clear(); self.status = format!("Selected {} × {} — hold Shift and drag to annotate", w, h); }
                    }
                    self.drag_rect = None;
                    self.drag_start = None;
                    self.annotating = false;
                }
                if let Some(r) = self.drag_rect { ui.painter().rect_stroke(r, 0.0, Stroke::new(2.0_f32, Color32::LIGHT_BLUE)); }
                if let Some([x,y,w,h]) = self.selected {
                    let r = Rect::from_min_size(rect.min + Vec2::new(x as f32, y as f32) * scale, Vec2::new(w as f32, h as f32) * scale);
                    ui.painter().rect_stroke(r, 0.0, Stroke::new(2.0_f32, Color32::GREEN));
                    for stroke in &self.strokes {
                        let color = match stroke.tool { Tool::Pen => Color32::from_rgb(0,120,212), Tool::Highlighter => Color32::from_rgba_unmultiplied(255,220,0,130), Tool::Eraser => Color32::TRANSPARENT };
                        let pts: Vec<Pos2> = stroke.points.iter().map(|p| Pos2::new(r.min.x+p[0]*r.width(), r.min.y+p[1]*r.height())).collect();
                        if pts.len() > 1 { ui.painter().add(egui::Shape::line(pts, Stroke::new(if stroke.tool == Tool::Highlighter { 9.0_f32 } else { 2.5_f32 }, color))); }
                    }
                }
            } else {
                ui.vertical_centered(|ui| { ui.add_space(90.0); ui.heading("Ready to capture"); ui.label("Capture your screen to select an area, save an image, or start a recording."); });
            }
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| { ui.label(&self.status); });
        let ended = self.recorder.as_mut().and_then(|r| r.child.try_wait().ok().flatten().map(|status| (status, r.temp_path.clone())));
        if let Some((status, path)) = ended {
            self.recorder.take();
            let valid = fs::metadata(&path).map(|m| m.len() > 1024).unwrap_or(false);
            self.status = if valid { format!("Recorder exited ({}). File preserved at {}", status, path.display()) }
                else { format!("Recorder stopped unexpectedly ({}). Check FFmpeg and capture permissions.", status) };
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
fn even(n: u32) -> u32 { if n % 2 == 0 { n } else { n + 1 } }
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
