use eframe::egui::{self, Color32, Stroke};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Obsidian,
    RoyalBlue,
    Bubblegum,
    AquaJade,
}

// These color roles are shared by the independently built Barracuda desktop apps.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Color32,
    pub surface: Color32,
    pub field: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub accent_text: Color32,
    pub secondary: Color32,
    pub secondary_text: Color32,
    pub selected: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub light: bool,
}

impl Theme {
    const ALL: [Self; 4] = [
        Self::Obsidian,
        Self::RoyalBlue,
        Self::Bubblegum,
        Self::AquaJade,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Obsidian => "Obsidian",
            Self::RoyalBlue => "Royal Blue",
            Self::Bubblegum => "Bubblegum",
            Self::AquaJade => "Aqua Jade",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Obsidian => "obsidian",
            Self::RoyalBlue => "royal-blue",
            Self::Bubblegum => "bubblegum",
            Self::AquaJade => "aqua-jade",
        }
    }

    fn colors(self) -> Colors {
        let (background, accent, secondary, text, on_accent) = match self {
            Self::Obsidian => (0x111217, 0xEF233C, 0xDFE5EB, 0xDFE5EB, 0x08090B),
            Self::RoyalBlue => (0x21232E, 0x105BFF, 0xFE8500, 0xF1F4FA, 0xFFFFFF),
            Self::Bubblegum => (0xF0F0F0, 0x105BFF, 0xFFC1CC, 0x202538, 0xFFFFFF),
            Self::AquaJade => (0x171F44, 0x74FFED, 0xFE81D1, 0xEAF0FF, 0x171F44),
        };
        let [background, accent, secondary, text, on_accent] =
            [background, accent, secondary, text, on_accent].map(rgb);
        let light = self == Self::Bubblegum;
        let surface = if light {
            Color32::WHITE
        } else {
            mix(background, text, 7)
        };
        Colors {
            background,
            surface,
            field: if light {
                rgb(0xF7F8FA)
            } else {
                mix(background, text, 4)
            },
            border: mix(background, text, 23),
            text,
            muted: mix(background, text, 72),
            accent,
            on_accent,
            accent_text: match self {
                Self::RoyalBlue => mix(accent, text, 40),
                Self::Obsidian => mix(accent, text, 20),
                _ => accent,
            },
            secondary,
            secondary_text: if light {
                mix(secondary, text, 58)
            } else {
                secondary
            },
            selected: mix(surface, accent, 19),
            success: rgb(if light { 0x176343 } else { 0x8ADDAE }),
            warning: rgb(if light { 0x854600 } else { 0xF5C778 }),
            error: rgb(if light { 0xA62B3E } else { 0xFF9FA4 }),
            light,
        }
    }
}

fn rgb(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

pub fn mix(base: Color32, tint: Color32, amount: u16) -> Color32 {
    let blend =
        |a: u8, b: u8| ((u16::from(a) * (100 - amount) + u16::from(b) * amount) / 100) as u8;
    Color32::from_rgb(
        blend(base.r(), tint.r()),
        blend(base.g(), tint.g()),
        blend(base.b(), tint.b()),
    )
}

fn path(app: &str) -> io::Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("Barracuda Systems").join(app).join("theme.txt"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA"))
}

fn theme_id() -> egui::Id {
    egui::Id::unique("barracuda-theme")
}

fn active(ctx: &egui::Context) -> Theme {
    ctx.data(|data| data.get_temp(theme_id()))
        .unwrap_or_default()
}

pub fn colors(ctx: &egui::Context) -> Colors {
    active(ctx).colors()
}

pub fn load(ctx: &egui::Context, app: &str) {
    let theme = path(app)
        .ok()
        .map_or_else(Theme::default, |file| read(&file));
    apply(ctx, theme);
}

fn read(file: &Path) -> Theme {
    fs::read_to_string(file)
        .ok()
        .and_then(|saved| {
            Theme::ALL
                .into_iter()
                .find(|theme| theme.key() == saved.trim())
        })
        .unwrap_or_default()
}

fn save(file: &Path, theme: Theme) -> io::Result<()> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    fs::create_dir_all(file.parent().expect("theme path has a directory"))?;
    let temporary = file.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = output.write_all(theme.key().as_bytes());
    drop(output);
    let result = written.and_then(|()| fs::rename(&temporary, file));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn apply(ctx: &egui::Context, theme: Theme) {
    ctx.data_mut(|data| data.insert_temp(theme_id(), theme));
    let colors = theme.colors();
    let style = (*ctx.global_style()).clone();
    let (mode, mut visuals) = if colors.light {
        (egui::Theme::Light, egui::Visuals::light())
    } else {
        (egui::Theme::Dark, egui::Visuals::dark())
    };
    visuals.override_text_color = Some(colors.text);
    visuals.weak_text_color = Some(colors.muted);
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.window_stroke = Stroke::new(1.0, colors.border);
    visuals.extreme_bg_color = colors.field;
    visuals.faint_bg_color = colors.surface;
    visuals.code_bg_color = colors.field;
    visuals.hyperlink_color = colors.accent_text;
    visuals.warn_fg_color = colors.warning;
    visuals.error_fg_color = colors.error;
    visuals.selection.bg_fill = colors.selected;
    visuals.selection.stroke = Stroke::new(1.0, colors.text);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = colors.surface;
        widget.weak_bg_fill = colors.surface;
        widget.bg_stroke.color = colors.border;
        widget.fg_stroke.color = colors.text;
    }
    for widget in [
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = colors.selected;
        widget.weak_bg_fill = colors.selected;
        widget.bg_stroke.color = colors.accent_text;
    }
    ctx.set_theme(mode);
    ctx.set_global_style(egui::Style { visuals, ..style });
    ctx.request_repaint();
}

pub fn picker(ui: &mut egui::Ui, app: &str, label: &str) -> bool {
    let before = active(ui.ctx());
    let mut selected = before;
    ui.strong(label);
    for theme in Theme::ALL {
        ui.horizontal(|ui| {
            let colors = theme.colors();
            for color in [colors.background, colors.accent, colors.secondary] {
                ui.label(egui::RichText::new("■").color(color));
            }
            ui.selectable_value(&mut selected, theme, theme.name());
        });
    }
    let error_id = egui::Id::unique("barracuda-theme-save-error");
    if selected != before {
        match path(app).and_then(|file| save(&file, selected)) {
            Ok(()) => {
                ui.ctx().data_mut(|data| data.remove::<String>(error_id));
                apply(ui.ctx(), selected);
                return true;
            }
            Err(error) => {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(error_id, error.to_string()));
            }
        }
    }
    if let Some(error) = ui.ctx().data(|data| data.get_temp::<String>(error_id)) {
        ui.colored_label(colors(ui.ctx()).error, format!("{label}: {error}"));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_saved_palettes_apply_to_a_new_context() {
        let directory =
            std::env::temp_dir().join(format!("barracuda-theme-{}", std::process::id()));
        let file = directory.join("theme.txt");
        assert_eq!(read(&file), Theme::Obsidian);
        for theme in Theme::ALL {
            save(&file, theme).unwrap();
            let ctx = egui::Context::default();
            apply(&ctx, read(&file));
            assert_eq!(active(&ctx), theme);
            assert_eq!(
                ctx.global_style().visuals.panel_fill,
                theme.colors().background
            );
            assert_eq!(ctx.global_style().visuals.dark_mode, !theme.colors().light);
        }
        fs::write(&file, "unrecognized-theme").unwrap();
        assert_eq!(read(&file), Theme::Obsidian);
        fs::remove_file(&file).unwrap();
        fs::remove_dir(&directory).unwrap();
    }
}
