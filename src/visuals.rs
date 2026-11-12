use crate::monitor::{Connection, Transfer};
use crate::native::FIN;
use crate::theme::{self, Colors};
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Frame, Margin, Painter, Rect, RichText, Shape,
    Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::path::PathBuf;

pub const ROW: f32 = 43.0;
pub const HOP: f32 = 49.0;
pub const GUTTER: f32 = 11.0;
pub const TRAFFIC_HEIGHT: f32 = 168.0;

pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut bold_fonts = fonts.families[&FontFamily::Proportional].clone();
    if let Some(windows) = std::env::var_os("WINDIR") {
        let folder = PathBuf::from(windows).join("Fonts");
        for (file, name) in [
            ("segoeui.ttf", "Segoe UI"),
            ("segoeuib.ttf", "Segoe UI Bold"),
        ] {
            if let Ok(data) = std::fs::read(folder.join(file)) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(data).into());
                if file == "segoeui.ttf" {
                    fonts
                        .families
                        .get_mut(&FontFamily::Proportional)
                        .unwrap()
                        .insert(0, name.into());
                } else {
                    bold_fonts.insert(0, name.into());
                }
            }
        }
    }
    fonts.families.insert(bold(14.0).family, bold_fonts);
    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = vec2(8.0, 4.0);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(15.0));
    ctx.set_global_style(style);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Barracuda Bold".into()))
}

pub fn panel(colors: Colors) -> Frame {
    Frame::new()
        .fill(colors.surface)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(10)
        .inner_margin(Margin::same(16))
}

pub fn heading(ui: &mut Ui, title: &str, description: &str) {
    let colors = theme::colors(ui.ctx());
    ui.label(RichText::new(title).font(bold(26.0)));
    ui.label(RichText::new(description).color(colors.muted));
}

pub fn brand_bar(ui: &mut Ui, connections: usize, ready: bool) {
    let colors = theme::colors(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 55.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, colors.surface);
    p.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, colors.border),
    );
    let mark = Rect::from_center_size(rect.left_center() + vec2(37.0, 0.0), vec2(42.0, 36.0));
    for (triangle, color) in FIN.iter().zip([colors.accent, colors.secondary]) {
        p.add(Shape::convex_polygon(
            triangle
                .iter()
                .map(|[x, y]| {
                    pos2(
                        mark.left() + x * mark.width(),
                        mark.top() + y * mark.height(),
                    )
                })
                .collect(),
            color,
            Stroke::NONE,
        ));
    }
    p.text(
        rect.left_center() + vec2(65.0, 0.0),
        Align2::LEFT_CENTER,
        "Barracuda",
        bold(23.0),
        colors.text,
    );
    p.text(
        rect.left_center() + vec2(181.0, 0.0),
        Align2::LEFT_CENTER,
        "Systems",
        FontId::proportional(19.0),
        colors.muted,
    );
    let x = rect.left() + 266.0;
    p.line_segment(
        [pos2(x, rect.top() + 13.0), pos2(x, rect.bottom() - 13.0)],
        Stroke::new(1.0, colors.border),
    );
    p.text(
        pos2(x + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Network Watch",
        FontId::proportional(18.0),
        colors.text,
    );

    let status = if ready {
        format!("{connections} TCP bağlantısı")
    } else {
        "Bağlantılar okunuyor".into()
    };
    let galley = p.layout_no_wrap(status, FontId::proportional(14.0), colors.muted);
    let status_pos = rect.right_center() - vec2(21.0 + galley.size().x, galley.size().y / 2.0);
    p.circle_filled(
        pos2(status_pos.x - 12.0, rect.center().y),
        4.0,
        if ready {
            colors.success
        } else {
            colors.warning
        },
    );
    p.galley(status_pos, galley, colors.muted);
}

pub fn search_icon(painter: &Painter, center: egui::Pos2, color: Color32) {
    let origin = center - vec2(1.0, 1.0);
    let stroke = Stroke::new(1.6, color);
    painter.circle_stroke(origin, 5.0, stroke);
    painter.line_segment([origin + vec2(3.6, 3.6), origin + vec2(7.0, 7.0)], stroke);
}

pub fn arrow(painter: &Painter, center: egui::Pos2, up: bool, color: Color32) {
    let direction = if up { -1.0 } else { 1.0 };
    let stroke = Stroke::new(1.55, color);
    let tip = center + vec2(0.0, 6.0 * direction);
    painter.line_segment([center - vec2(0.0, 5.5 * direction), tip], stroke);
    painter.line_segment([tip, tip + vec2(-3.5, -3.5 * direction)], stroke);
    painter.line_segment([tip, tip + vec2(3.5, -3.5 * direction)], stroke);
}

pub fn cell_text(
    painter: &Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    right: bool,
) {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), font, color, rect.width().max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let x = if right {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    painter.with_clip_rect(rect).galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

pub fn transfer_card(ui: &mut Ui, connection: Option<&Connection>) {
    let colors = theme::colors(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), TRAFFIC_HEIGHT),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect(
        rect,
        9.0,
        colors.field,
        Stroke::new(1.0, colors.border),
        StrokeKind::Inside,
    );
    p.rect_filled(
        Rect::from_min_max(
            rect.min + vec2(1.0, 10.0),
            pos2(rect.left() + 4.0, rect.bottom() - 10.0),
        ),
        1.5,
        colors.accent,
    );
    let inset = rect.shrink2(vec2(16.0, 11.0));
    p.text(
        inset.left_top(),
        Align2::LEFT_TOP,
        "Seçili bağlantının trafiği",
        bold(20.0),
        colors.text,
    );
    let detail = connection.map_or(
        "Hız geçmişi ve toplam veri için bir bağlantı seç.".into(),
        |c| format!("{}  ·  {}", c.process.name, c.key.remote),
    );
    cell_text(
        p,
        Rect::from_min_size(
            inset.left_top() + vec2(0.0, 25.0),
            vec2(inset.width(), 21.0),
        ),
        &detail,
        FontId::proportional(14.0),
        colors.muted,
        false,
    );
    let divider = rect.top() + 64.0;
    p.hline(inset.x_range(), divider, Stroke::new(1.0, colors.border));
    let plot_left = inset.left() + (inset.width() * 0.49).max(231.0);
    p.line_segment(
        [
            pos2(plot_left - 10.0, divider + 10.0),
            pos2(plot_left - 10.0, inset.bottom()),
        ],
        Stroke::new(1.0, colors.border),
    );
    let transfer = connection.map(|c| &c.transfer);
    p.text(
        pos2(inset.left(), divider + 9.0),
        Align2::LEFT_TOP,
        "İzleme başlangıcından beri",
        FontId::proportional(13.0),
        colors.muted,
    );
    let metric_width = (plot_left - 12.0 - inset.left()) / 2.0;
    for (i, (label, color)) in [
        ("Toplam indirilen", colors.accent_text),
        ("Toplam yüklenen", colors.secondary_text),
    ]
    .into_iter()
    .enumerate()
    {
        let x = inset.left() + i as f32 * metric_width;
        p.text(
            pos2(x, divider + 31.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(13.0),
            colors.muted,
        );
        let value = transfer
            .filter(|t| t.has_counters())
            .map_or("—".into(), |t| {
                quantity(
                    if i == 0 { t.totals.down } else { t.totals.up } as f64,
                    false,
                )
            });
        cell_text(
            p,
            Rect::from_min_size(pos2(x, divider + 50.0), vec2(metric_width - 6.0, 32.0)),
            &value,
            bold(25.0),
            color,
            false,
        );
    }
    let plot = Rect::from_min_max(pos2(plot_left, divider + 8.0), inset.right_bottom());
    chart(p, plot, transfer, colors);
}

fn chart(p: &Painter, rect: Rect, transfer: Option<&Transfer>, colors: Colors) {
    p.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        "Son 60 saniye",
        bold(14.0),
        colors.text,
    );
    for (index, color) in [colors.secondary_text, colors.accent_text]
        .into_iter()
        .enumerate()
    {
        arrow(
            p,
            rect.right_top() - vec2(7.0 + index as f32 * 19.0, -9.0),
            index == 0,
            color,
        );
    }
    let Some(transfer) = transfer.filter(|t| t.history.iter().any(|s| s.rate.is_some())) else {
        let area = Rect::from_min_max(rect.left_top() + vec2(0.0, 24.0), rect.right_bottom());
        p.rect(
            area,
            5.0,
            colors.surface,
            Stroke::new(1.0, colors.border),
            StrokeKind::Inside,
        );
        cell_text(
            p,
            area.shrink(8.0),
            "Hız verisi bekleniyor",
            FontId::proportional(13.0),
            colors.muted,
            false,
        );
        return;
    };
    let latest = transfer.history.back().unwrap().time;
    let max = transfer
        .history
        .iter()
        .filter_map(|s| s.rate)
        .map(|r| r.down.max(r.up))
        .fold(1000.0_f64, f64::max);
    let graph = Rect::from_min_max(rect.min + vec2(62.0, 26.0), rect.max - vec2(0.0, 3.0));
    p.text(
        graph.left_top() - vec2(5.0, 0.0),
        Align2::RIGHT_TOP,
        quantity(max, true),
        FontId::proportional(11.0),
        colors.muted,
    );
    p.text(
        graph.left_bottom() - vec2(5.0, 0.0),
        Align2::RIGHT_BOTTOM,
        "0",
        FontId::proportional(11.0),
        colors.muted,
    );
    for y in [graph.top(), graph.center().y, graph.bottom()] {
        p.hline(graph.x_range(), y, Stroke::new(1.0, colors.border));
    }
    for (upload, color) in [(false, colors.accent_text), (true, colors.secondary_text)] {
        let mut last = None;
        for sample in &transfer.history {
            let point = sample.rate.map(|rate| {
                let age = (latest - sample.time).as_secs_f32();
                let value = if upload { rate.up } else { rate.down };
                pos2(
                    graph.right() - (age / 60.0).min(1.0) * graph.width(),
                    graph.bottom() - (value / max) as f32 * graph.height(),
                )
            });
            if let Some(point) = point {
                if let Some(previous) = last {
                    p.line_segment([previous, point], Stroke::new(1.8, color));
                } else {
                    p.circle_filled(point, 1.4, color);
                }
            }
            last = point;
        }
    }
}

pub fn quantity(value: f64, rate: bool) -> String {
    let (value, unit) = if value >= 1_000_000_000.0 {
        (format!("{:.2}", value / 1_000_000_000.0), "GB")
    } else if value >= 1_000_000.0 {
        (format!("{:.2}", value / 1_000_000.0), "MB")
    } else if value >= 1_000.0 || value == 0.0 && rate {
        (format!("{:.1}", value / 1_000.0), "KB")
    } else {
        (format!("{value:.0}"), "B")
    };
    format!(
        "{} {unit}{}",
        value.replace('.', ","),
        if rate { "/sn" } else { "" }
    )
}
