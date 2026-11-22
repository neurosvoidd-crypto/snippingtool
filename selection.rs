//! One native, full-desktop rectangle picker shared by screenshots and recordings.
//! The underlying desktop is captured *before* this overlay is shown; the picker
//! draws that still image, dimmed, so it never appears in the recording itself.

use image::RgbaImage;

/// Pixel-space rectangle, relative to the combined screen image.
pub type PixelRect = [u32; 4];

#[cfg(windows)]
mod win32 {
    use super::{PixelRect, RgbaImage};
    use std::{ffi::c_void, mem, ptr};

    type Handle = *mut c_void;
    type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const SW_HIDE: i32 = 0;
    const SW_SHOW: i32 = 5;
    const WM_DESTROY: u32 = 0x0002;
    const WM_PAINT: u32 = 0x000F;
    const WM_SETCURSOR: u32 = 0x0020;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_MOUSEMOVE: u32 = 0x0200;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;
    const VK_ESCAPE: usize = 0x1B;
    const GWLP_USERDATA: i32 = -21;
    const PS_SOLID: i32 = 0;
    const NULL_BRUSH: i32 = 5;
    const TRANSPARENT: i32 = 1;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;
    const SRCCOPY: u32 = 0x00CC_0020;
    const IDC_CROSS: usize = 32515;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point { x: i32, y: i32 }
    #[repr(C)]
    #[derive(Default)]
    struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
    #[repr(C)]
    struct WindowClass {
        style: u32,
        wnd_proc: Option<WndProc>,
        class_extra: i32,
        window_extra: i32,
        instance: Handle,
        icon: Handle,
        cursor: Handle,
        background: Handle,
        menu_name: *const u16,
        class_name: *const u16,
    }
    #[repr(C)]
    struct PaintStruct {
        dc: Handle,
        erase: i32,
        paint_rect: Rect,
        restore: i32,
        inc_update: i32,
        reserved: [u8; 32],
    }
    #[repr(C)]
    struct Message {
        hwnd: Handle,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
        private: u32,
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        xppm: i32,
        yppm: i32,
        colors_used: u32,
        colors_important: u32,
    }
    #[repr(C)]
    struct RgbQuad { blue: u8, green: u8, red: u8, reserved: u8 }
    #[repr(C)]
    struct BitmapInfo { header: BitmapInfoHeader, colors: [RgbQuad; 1] }

    #[link(name = "user32")]
    extern "system" {
        fn RegisterClassW(class: *const WindowClass) -> u16;
        fn CreateWindowExW(ex_style: u32, class: *const u16, title: *const u16,
            style: u32, x: i32, y: i32, width: i32, height: i32,
            parent: Handle, menu: Handle, instance: Handle, param: *mut c_void) -> Handle;
        fn DefWindowProcW(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn DestroyWindow(hwnd: Handle) -> i32;
        fn ShowWindow(hwnd: Handle, cmd: i32) -> i32;
        fn SetForegroundWindow(hwnd: Handle) -> i32;
        fn GetForegroundWindow() -> Handle;
        fn GetWindowLongPtrW(hwnd: Handle, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: Handle, index: i32, data: isize) -> isize;
        fn GetMessageW(msg: *mut Message, hwnd: Handle, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Message) -> i32;
        fn DispatchMessageW(msg: *const Message) -> isize;
        fn PostQuitMessage(code: i32);
        fn BeginPaint(hwnd: Handle, ps: *mut PaintStruct) -> Handle;
        fn EndPaint(hwnd: Handle, ps: *const PaintStruct) -> i32;
        fn InvalidateRect(hwnd: Handle, rect: *const Rect, erase: i32) -> i32;
        fn SetCapture(hwnd: Handle) -> Handle;
        fn ReleaseCapture() -> i32;
        fn LoadCursorW(instance: Handle, cursor: *const u16) -> Handle;
        fn SetCursor(cursor: Handle) -> Handle;
    }
    #[link(name = "kernel32")]
    extern "system" { fn GetModuleHandleW(name: *const u16) -> Handle; }
    #[link(name = "gdi32")]
    extern "system" {
        fn StretchDIBits(dc: Handle, dx: i32, dy: i32, dw: i32, dh: i32,
            sx: i32, sy: i32, sw: i32, sh: i32, bits: *const c_void,
            bmi: *const BitmapInfo, usage: u32, operation: u32) -> i32;
        fn CreatePen(style: i32, width: i32, color: u32) -> Handle;
        fn SelectObject(dc: Handle, object: Handle) -> Handle;
        fn DeleteObject(object: Handle) -> i32;
        fn GetStockObject(index: i32) -> Handle;
        fn Rectangle(dc: Handle, left: i32, top: i32, right: i32, bottom: i32) -> i32;
        fn SetBkMode(dc: Handle, mode: i32) -> i32;
        fn SetTextColor(dc: Handle, color: u32) -> u32;
        fn TextOutW(dc: Handle, x: i32, y: i32, text: *const u16, len: i32) -> i32;
    }

    struct Picker {
        width: i32,
        height: i32,
        full: Vec<u8>,
        dim: Vec<u8>,
        dragging: bool,
        start: Point,
        end: Point,
        result: Option<PixelRect>,
        mode: &'static str,
    }

    impl Picker {
        fn box_rect(&self) -> Rect {
            Rect {
                left: self.start.x.min(self.end.x).clamp(0, self.width),
                top: self.start.y.min(self.end.y).clamp(0, self.height),
                right: self.start.x.max(self.end.x).clamp(0, self.width),
                bottom: self.start.y.max(self.end.y).clamp(0, self.height),
            }
        }
    }

    fn pointer_point(lparam: isize) -> Point {
        Point { x: (lparam as u16 as i16) as i32, y: ((lparam >> 16) as u16 as i16) as i32 }
    }

    unsafe fn draw_bitmap(dc: Handle, picker: &Picker, pixels: &[u8], crop: Option<&Rect>) {
        let bmi = BitmapInfo {
            header: BitmapInfoHeader {
                size: mem::size_of::<BitmapInfoHeader>() as u32,
                width: picker.width,
                height: -picker.height, // top-down memory order
                planes: 1,
                bit_count: 32,
                compression: BI_RGB,
                size_image: 0,
                xppm: 0,
                yppm: 0,
                colors_used: 0,
                colors_important: 0,
            },
            colors: [RgbQuad { blue: 0, green: 0, red: 0, reserved: 0 }],
        };
        let r = crop.map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top))
            .unwrap_or((0, 0, picker.width, picker.height));
        if r.2 > 0 && r.3 > 0 {
            StretchDIBits(dc, r.0, r.1, r.2, r.3, r.0, r.1, r.2, r.3,
                pixels.as_ptr().cast(), &bmi, DIB_RGB_COLORS, SRCCOPY);
        }
    }

    unsafe fn draw_label(dc: Handle, x: i32, y: i32, label: &str) {
        let utf16: Vec<u16> = label.encode_utf16().collect();
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, 0x00FF_FFFF);
        TextOutW(dc, x, y, utf16.as_ptr(), utf16.len() as i32);
    }

    unsafe extern "system" fn window_proc(hwnd: Handle, msg: u32, wparam: usize, lparam: isize) -> isize {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Picker;
        match msg {
            WM_SETCURSOR => {
                SetCursor(LoadCursorW(ptr::null_mut(), IDC_CROSS as *const u16));
                return 1;
            }
            WM_PAINT => {
                let mut ps: PaintStruct = mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                if !state.is_null() && !dc.is_null() {
                    let picker = &*state;
                    draw_bitmap(dc, picker, &picker.dim, None);
                    if picker.dragging {
                        let r = picker.box_rect();
                        draw_bitmap(dc, picker, &picker.full, Some(&r));
                        let pen = CreatePen(PS_SOLID, 2, 0x00D4_7800); // blue in COLORREF BGR
                        if !pen.is_null() {
                            let old_pen = SelectObject(dc, pen);
                            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                            Rectangle(dc, r.left, r.top, r.right, r.bottom);
                            SelectObject(dc, old_brush);
                            SelectObject(dc, old_pen);
                            DeleteObject(pen);
                        }
                        draw_label(dc, (r.left + 8).min(picker.width - 140).max(4),
                            (r.top - 23).max(6),
                            &format!("{} x {}", r.right - r.left, r.bottom - r.top));
                    }
                    draw_label(dc, 20, 18, &format!("SnipStudio {} | Drag to select | Esc or right-click to cancel", picker.mode));
                }
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_LBUTTONDOWN if !state.is_null() => {
                let picker = &mut *state;
                picker.dragging = true;
                picker.start = pointer_point(lparam);
                picker.end = picker.start;
                SetCapture(hwnd);
                InvalidateRect(hwnd, ptr::null(), 0);
                return 0;
            }
            WM_MOUSEMOVE if !state.is_null() => {
                let picker = &mut *state;
                if picker.dragging {
                    picker.end = pointer_point(lparam);
                    InvalidateRect(hwnd, ptr::null(), 0);
                }
                return 0;
            }
            WM_LBUTTONUP if !state.is_null() => {
                let picker = &mut *state;
                if picker.dragging {
                    picker.end = pointer_point(lparam);
                    let r = picker.box_rect();
                    picker.dragging = false;
                    ReleaseCapture();
                    if r.right - r.left >= 2 && r.bottom - r.top >= 2 {
                        picker.result = Some([r.left as u32, r.top as u32,
                            (r.right - r.left) as u32, (r.bottom - r.top) as u32]);
                        DestroyWindow(hwnd);
                    } else {
                        InvalidateRect(hwnd, ptr::null(), 0);
                    }
                }
                return 0;
            }
            WM_KEYDOWN if wparam == VK_ESCAPE => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_RBUTTONUP => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Hide/restore the main app while taking the background screenshot.
    /// Caller must hold this guard through the modal desktop picker.
    pub struct HiddenApp { hwnd: Handle }
    impl HiddenApp {
        pub fn new() -> Self {
            unsafe {
                let hwnd = GetForegroundWindow();
                if !hwnd.is_null() { ShowWindow(hwnd, SW_HIDE); }
                Self { hwnd }
            }
        }
    }
    impl Drop for HiddenApp {
        fn drop(&mut self) {
            unsafe {
                if !self.hwnd.is_null() {
                    ShowWindow(self.hwnd, SW_SHOW);
                    SetForegroundWindow(self.hwnd);
                }
            }
        }
    }

    pub fn select_area(image: &RgbaImage, screen_origin: (i32, i32), mode: &'static str) -> Result<Option<PixelRect>, String> {
        let w = i32::try_from(image.width()).map_err(|e| e.to_string())?;
        let h = i32::try_from(image.height()).map_err(|e| e.to_string())?;
        let mut full = Vec::with_capacity(image.as_raw().len());
        let mut dim = Vec::with_capacity(image.as_raw().len());
        for pixel in image.as_raw().chunks_exact(4) {
            full.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]); // RGBA -> GDI BGRA
            dim.extend_from_slice(&[
                (u16::from(pixel[2]) * 48 / 100) as u8,
                (u16::from(pixel[1]) * 48 / 100) as u8,
                (u16::from(pixel[0]) * 48 / 100) as u8,
                255,
            ]);
        }
        let mut picker = Box::new(Picker {
            width: w, height: h, full, dim, dragging: false,
            start: Point::default(), end: Point::default(), result: None, mode,
        });
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class_name: Vec<u16> = "SnipStudioDesktopRectanglePicker\0".encode_utf16().collect();
            let title: Vec<u16> = "SnipStudio Select Area\0".encode_utf16().collect();
            RegisterClassW(&WindowClass {
                style: 0, wnd_proc: Some(window_proc), class_extra: 0, window_extra: 0,
                instance, icon: ptr::null_mut(), cursor: ptr::null_mut(),
                background: ptr::null_mut(), menu_name: ptr::null(), class_name: class_name.as_ptr(),
            }); // Already-registered class is fine for subsequent captures.
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class_name.as_ptr(), title.as_ptr(),
                WS_POPUP, screen_origin.0, screen_origin.1, w, h,
                ptr::null_mut(), ptr::null_mut(), instance, ptr::null_mut(),
            );
            if hwnd.is_null() {
                return Err(format!("Unable to create desktop selection overlay: {}", std::io::Error::last_os_error()));
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *picker as *mut Picker) as isize);
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            InvalidateRect(hwnd, ptr::null(), 0);
            loop {
                let mut msg: Message = mem::zeroed();
                let code = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
                if code == 0 { break; } // The overlay consumed its own WM_QUIT.
                if code < 0 {
                    DestroyWindow(hwnd);
                    // WM_DESTROY posts WM_QUIT. Consume it before returning to eframe.
                    let mut quit: Message = mem::zeroed();
                    let _ = GetMessageW(&mut quit, ptr::null_mut(), 0, 0);
                    return Err("Windows message loop failed during selection".into());
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Ok(picker.result)
    }
}

#[cfg(windows)]
pub use win32::{select_area, HiddenApp};

#[cfg(not(windows))]
pub struct HiddenApp;
#[cfg(not(windows))]
impl HiddenApp { pub fn new() -> Self { Self } }

#[cfg(not(windows))]
pub fn select_area(_image: &RgbaImage, _origin: (i32, i32), _mode: &'static str)
    -> Result<Option<PixelRect>, String> {
    Err("The full-desktop rectangle picker is available on Windows only".into())
}
