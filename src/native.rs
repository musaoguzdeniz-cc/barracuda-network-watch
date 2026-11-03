use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::{HashMap, HashSet};
use std::mem::size_of_val;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::mpsc;
use std::thread;
use windows_sys::Win32::Graphics::{Dwm::*, Gdi::*};
use windows_sys::Win32::UI::{
    Shell::ExtractIconExW,
    WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
};

pub const FIN: [[[f32; 2]; 3]; 2] = [
    [[0.05, 0.70], [0.94, 0.16], [0.64, 0.54]],
    [[0.05, 0.70], [0.64, 0.54], [0.27, 0.87]],
];
pub const FIN_COLORS: [Color32; 2] = [
    Color32::from_rgb(237, 39, 79),
    Color32::from_rgb(181, 21, 57),
];

pub fn dark_caption(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as _;
    for (attribute, value) in [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, 1),
        (DWMWA_CAPTION_COLOR, color_ref(28, 29, 33)),
        (DWMWA_TEXT_COLOR, color_ref(244, 244, 247)),
        (DWMWA_BORDER_COLOR, color_ref(58, 59, 65)),
    ] {
        // DWM rejects unsupported attributes on older Windows builds.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                (&raw const value).cast(),
                size_of_val(&value) as u32,
            )
        };
    }
}

const fn color_ref(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

pub fn app_icon() -> egui::IconData {
    const SIZE: u32 = 48;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = [
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            ];
            let color = FIN
                .iter()
                .zip(FIN_COLORS)
                .find_map(|(triangle, color)| {
                    let edges = (0..3).map(|i| {
                        let [ax, ay] = triangle[i];
                        let [bx, by] = triangle[(i + 1) % 3];
                        (bx - ax) * (point[1] - ay) - (by - ay) * (point[0] - ax)
                    });
                    (edges.clone().all(|edge| edge >= 0.0) || edges.clone().all(|edge| edge <= 0.0))
                        .then_some(color)
                })
                .unwrap_or(Color32::TRANSPARENT);
            rgba.extend(color.to_array());
        }
    }
    egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    }
}

pub struct Icons {
    textures: HashMap<PathBuf, Option<TextureHandle>>,
    pending: HashSet<PathBuf>,
    requests: mpsc::SyncSender<PathBuf>,
    results: mpsc::Receiver<(PathBuf, Option<ColorImage>)>,
}

impl Icons {
    pub fn new(ctx: egui::Context) -> Self {
        let (requests, paths) = mpsc::sync_channel::<PathBuf>(24);
        let (images, results) = mpsc::channel();
        thread::spawn(move || {
            for path in paths {
                let image = executable_icon(&path);
                if images.send((path, image)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            requests,
            results,
        }
    }

    pub fn refresh(&mut self, ctx: &egui::Context) {
        for (path, image) in self.results.try_iter() {
            self.pending.remove(&path);
            if self.textures.len() >= 256 {
                self.textures.clear();
            }
            self.textures.insert(
                path,
                image.map(|image| {
                    ctx.load_texture("Windows app icon", image, TextureOptions::LINEAR)
                }),
            );
        }
    }

    pub fn get(&mut self, path: Option<&Path>) -> Option<egui::TextureId> {
        let path = path?;
        if let Some(entry) = self.textures.get(path) {
            return entry.as_ref().map(TextureHandle::id);
        }
        if !self.pending.contains(path) && self.requests.try_send(path.to_owned()).is_ok() {
            self.pending.insert(path.to_owned());
        }
        None
    }
}

const ICON_SIZE: usize = 32;
const ICON_BYTES: usize = ICON_SIZE * ICON_SIZE * 4;

fn executable_icon(path: &Path) -> Option<ColorImage> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut icon = null_mut();
    // ExtractIconEx reads the executable's icon resource.
    let count = unsafe { ExtractIconExW(path.as_ptr(), 0, &mut icon, null_mut(), 1) };
    if count != 1 || icon.is_null() {
        return None;
    }
    let image = render_icon(icon);
    unsafe { DestroyIcon(icon) };
    image
}

fn render_icon(icon: HICON) -> Option<ColorImage> {
    let size = ICON_SIZE as i32;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of_val(&BITMAPINFOHEADER::default()) as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    // A memory DC and a top-down BGRA DIB provide a fixed-size drawing surface.
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let image = if !bitmap.is_null() && !bits.is_null() {
            let previous = SelectObject(dc, bitmap);
            if previous.is_null() {
                None
            } else {
                let draw = |background| {
                    let pixels = bits.cast::<u8>();
                    pixels.write_bytes(background, ICON_BYTES);
                    if DrawIconEx(dc, 0, 0, icon, size, size, 0, null_mut(), DI_NORMAL) == 0 {
                        return None;
                    }
                    GdiFlush();
                    Some(std::slice::from_raw_parts(pixels, ICON_BYTES).to_vec())
                };
                // Reconstruct alpha from black and white composites, including
                // legacy icons that only carry a one-bit transparency mask.
                let image = draw(0).zip(draw(255)).map(|(mut dark, light)| {
                    for (dark, light) in dark
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(light.as_chunks::<4>().0)
                    {
                        let remaining = (0..3)
                            .map(|i| light[i].saturating_sub(dark[i]))
                            .max()
                            .unwrap_or(255);
                        dark.swap(0, 2);
                        dark[3] = 255 - remaining;
                    }
                    ColorImage::from_rgba_premultiplied([ICON_SIZE, ICON_SIZE], &dark)
                });
                SelectObject(dc, previous);
                image
            }
        } else {
            None
        };
        if !bitmap.is_null() {
            DeleteObject(bitmap);
        }
        DeleteDC(dc);
        image
    }
}
