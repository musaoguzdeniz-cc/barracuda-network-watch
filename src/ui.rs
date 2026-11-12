use crate::monitor::{self, Connection, Key, Monitor, Snapshot};
use crate::native::{self, Icons};
use crate::route::{self, Event, Finish, Hop};
use crate::visuals::*;
use crate::{THEME_APP, theme};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Layout, Margin, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::net::{IpAddr, SocketAddr};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub struct Watch {
    pending: Arc<Mutex<Option<Snapshot>>>,
    stop: Arc<AtomicBool>,
    snapshot: Snapshot,
    waiting: bool,
    search: String,
    selected: Option<Key>,
    route: Option<Trace>,
    intel: Option<Intel>,
    icons: Icons,
    trace_sequence: u64,
}

struct Trace {
    local: SocketAddr,
    target: SocketAddr,
    receiver: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
    hops: Vec<Hop>,
    sequence: u64,
    finish: Option<Finish>,
}

struct Intel {
    ip: IpAddr,
    receiver: mpsc::Receiver<Result<String, String>>,
    result: Option<Result<String, String>>,
}

impl Watch {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup(&cc.egui_ctx);
        theme::load(&cc.egui_ctx, THEME_APP);
        native::caption(cc, theme::colors(&cc.egui_ctx));

        let pending = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let mailbox = Arc::clone(&pending);
        let worker_stop = Arc::clone(&stop);
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            let mut monitor = Monitor::default();
            while !worker_stop.load(Ordering::Relaxed) {
                let started = Instant::now();
                let snapshot = monitor.poll();
                *mailbox.lock().unwrap() = Some(snapshot);
                ctx.request_repaint();
                thread::sleep(Duration::from_secs(1).saturating_sub(started.elapsed()));
            }
        });

        Self {
            pending,
            stop,
            snapshot: Snapshot::default(),
            waiting: true,
            search: String::new(),
            selected: None,
            route: None,
            intel: None,
            icons: Icons::new(cc.egui_ctx.clone()),
            trace_sequence: 0,
        }
    }

    fn start_trace(&mut self, key: Key, ctx: &egui::Context) {
        if self
            .route
            .as_ref()
            .is_some_and(|route| route.finish.is_none())
        {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let finish = route::trace(key.local, key.remote, &worker_cancel, |hop| {
                let _ = sender.send(Event::Hop(hop));
                ctx.request_repaint();
            });
            let _ = sender.send(Event::Done(finish));
            ctx.request_repaint();
        });
        self.trace_sequence = self.trace_sequence.wrapping_add(1);
        self.route = Some(Trace {
            sequence: self.trace_sequence,
            local: key.local,
            target: key.remote,
            receiver,
            cancel,
            hops: Vec::new(),
            finish: None,
        });
    }

    fn start_intel(&mut self, ip: IpAddr, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(lookup(ip));
            ctx.request_repaint();
        });
        self.intel = Some(Intel {
            ip,
            receiver,
            result: None,
        });
    }

    fn connections(&mut self, ui: &mut Ui) {
        let colors = theme::colors(ui.ctx());
        heading(
            ui,
            "Canlı bağlantılar",
            "Uygulamaların TCP bağlantıları ve anlık veri trafiği",
        );
        ui.add_space(10.0);
        let search = ui.add_sized(
            [ui.available_width(), 38.0],
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Uygulama, IP, port veya PID ara")
                .margin(Margin {
                    left: 35,
                    right: 12,
                    top: 8,
                    bottom: 8,
                }),
        );
        search_icon(
            ui.painter(),
            search.rect.left_center() + vec2(18.0, 0.0),
            colors.muted,
        );
        ui.add_space(8.0);

        if !self.snapshot.table_errors.is_empty() {
            ui.colored_label(
                colors.warning,
                format!(
                    "Bağlantı tablosu okunamadı · {}",
                    self.snapshot.table_errors.join(", "),
                ),
            );
        }
        if self
            .snapshot
            .connections
            .iter()
            .any(|c| c.traffic_error == Some(ERROR_ACCESS_DENIED))
        {
            ui.label(
                RichText::new("Canlı trafik sayaçları için yönetici olarak çalıştır.")
                    .color(colors.warning)
                    .size(13.0),
            );
        }

        let query = self.search.trim().to_lowercase();
        let rows: Vec<_> = self
            .snapshot
            .connections
            .iter()
            .filter(|c| {
                query.is_empty()
                    || c.process.name.to_lowercase().contains(&query)
                    || address(c.key.remote).contains(&query)
                    || c.key.remote.port().to_string().contains(&query)
                    || c.key.pid.to_string().contains(&query)
            })
            .collect();
        let width = ui.available_width() - GUTTER;
        table_heading(ui, width);
        let height = (ui.available_height() - TRAFFIC_HEIGHT - 12.0).max(ROW);
        let rows_height = (height / ROW).floor() * ROW;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("connections")
            .auto_shrink([false, false])
            .min_scrolled_height(0.0)
            .max_height(rows_height);
        if search.changed() {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            scroll.show_rows(ui, ROW, rows.len().max(1), |ui, range| {
                if rows.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                    cell_text(
                        ui.painter(),
                        rect.shrink(8.0),
                        if self.waiting {
                            "Bağlantılar okunuyor…"
                        } else {
                            "Aramaya uygun bağlantı bulunamadı."
                        },
                        FontId::proportional(14.0),
                        colors.muted,
                        false,
                    );
                    return;
                }
                for index in range {
                    let c = rows[index];
                    let icon = self.icons.get(c.process.image.as_deref());
                    if connection_row(ui, width, c, self.selected == Some(c.key), icon).clicked() {
                        self.selected = Some(c.key);
                    }
                }
            });
        });
        ui.add_space(4.0 + height - rows_height);
        transfer_card(ui, self.selected_connection());
    }

    fn selected_connection(&self) -> Option<&Connection> {
        self.snapshot
            .connections
            .iter()
            .find(|c| Some(c.key) == self.selected)
    }

    fn routes(&mut self, ui: &mut Ui) {
        let colors = theme::colors(ui.ctx());
        heading(
            ui,
            "Hedefe ölçülen rota",
            "Ayrı ICMP ölçümü · Gidiş-dönüş süresi",
        );
        ui.add_space(12.0);
        let busy = self
            .route
            .as_ref()
            .is_some_and(|route| route.finish.is_none());
        let selected = self.selected_connection().map(|c| c.key);
        let belongs = |route: &&Trace| {
            busy || selected
                .is_none_or(|key| route.local == key.local && route.target == key.remote)
        };
        let target = self
            .route
            .as_ref()
            .filter(belongs)
            .map(|route| route.target)
            .or(selected.map(|key| key.remote));
        Frame::new()
            .fill(colors.field)
            .stroke(Stroke::new(1.0, colors.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 34.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        if busy {
                            if ui
                                .add(route_button("Ölçümü durdur", false, colors))
                                .clicked()
                            {
                                self.route
                                    .as_ref()
                                    .unwrap()
                                    .cancel
                                    .store(true, Ordering::Relaxed);
                            }
                        } else if ui
                            .add_enabled(
                                selected.is_some(),
                                route_button("Rotayı ölç", true, colors),
                            )
                            .clicked()
                            && let Some(key) = selected
                        {
                            self.start_trace(key, ui.ctx());
                        }
                        let ip = target.map(|target| target.ip().to_canonical());
                        if ui
                            .add_enabled(
                                ip.is_some_and(is_public_candidate),
                                route_button("IP’yi incele", false, colors),
                            )
                            .on_hover_text("Barracuda IP Intelligence ile aç")
                            .clicked()
                            && let Some(ip) = ip
                        {
                            self.start_intel(ip, ui.ctx());
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            if let Some(target) = target {
                                let ip = address(target);
                                ui.add(
                                    egui::Label::new(RichText::new(&ip).font(bold(17.0)))
                                        .truncate(),
                                )
                                .on_hover_text(ip);
                            } else {
                                ui.label(RichText::new("Hedef seçilmedi").color(colors.muted));
                            }
                        });
                    },
                );
            });
        ui.add_space(12.0);

        let route = self.route.as_ref().filter(belongs);
        let height = (ui.available_height() - 69.0).max(62.0);
        let mut inspect = None;
        if let Some(route) = route {
            let width = ui.available_width() - GUTTER;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical()
                    .id_salt(("route hops", route.sequence))
                    .auto_shrink([false, false])
                    .min_scrolled_height(0.0)
                    .max_height(height)
                    .show_rows(ui, HOP, route.hops.len(), |ui, range| {
                        for index in range {
                            let reached = index + 1 == route.hops.len()
                                && matches!(route.finish, Some(Finish::Reached));
                            inspect = hop_row(
                                ui,
                                width,
                                &route.hops[index],
                                index + 1 == route.hops.len(),
                                reached,
                            )
                            .or(inspect);
                        }
                    });
            });
        } else {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
            let box_rect = Rect::from_center_size(rect.center(), vec2(46.0, 46.0));
            let painter = ui.painter();
            painter.rect(
                box_rect.translate(vec2(0.0, -33.0)),
                12.0,
                colors.field,
                Stroke::new(1.0, colors.border),
                StrokeKind::Inside,
            );
            let nodes = [vec2(-12.0, -27.0), vec2(0.0, -40.0), vec2(13.0, -31.0)]
                .map(|offset| rect.center() + offset);
            painter.add(egui::Shape::line(
                nodes.to_vec(),
                Stroke::new(1.6, colors.accent_text),
            ));
            for node in nodes {
                painter.circle_filled(node, 3.0, colors.accent_text);
            }
            painter.text(
                rect.center() + vec2(0.0, 4.0),
                Align2::CENTER_TOP,
                "Rota ölçümü",
                bold(17.0),
                colors.text,
            );
            let hint = if selected.is_some() {
                "Hop’ları görmek için Rotayı ölç düğmesine bas."
            } else {
                "Başlamak için bir TCP bağlantısı seç."
            };
            painter.text(
                rect.center() + vec2(0.0, 27.0),
                Align2::CENTER_TOP,
                hint,
                FontId::proportional(14.0),
                colors.muted,
            );
        }
        ui.add_space(8.0);
        route_status(ui, route);
        if let Some(ip) = inspect {
            self.start_intel(ip, ui.ctx());
        }
    }

    fn intel_window(&mut self, ui: &mut Ui) {
        let colors = theme::colors(ui.ctx());
        let Some(intel) = &mut self.intel else { return };
        if intel.result.is_none() {
            match intel.receiver.try_recv() {
                Ok(result) => intel.result = Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    intel.result = Some(Err("IP Intelligence işlemi tamamlanamadı.".into()));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut open = true;
        egui::Window::new(format!("IP Intelligence · {}", intel.ip))
            .open(&mut open)
            .default_size([650.0, 420.0])
            .show(ui.ctx(), |ui| match &mut intel.result {
                Some(Ok(report)) => {
                    if ui.button("Raporu kopyala").clicked() {
                        ui.ctx().copy_text(report.clone());
                    }
                    egui::ScrollArea::both().show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut report.as_str()).code_editor());
                    });
                }
                Some(Err(message)) => {
                    ui.colored_label(colors.warning, message.as_str());
                    ui.hyperlink_to(
                        "Barracuda IP Intelligence",
                        "https://github.com/musaoguzdeniz-cc/barracuda-ip-intelligence",
                    );
                }
                None => {
                    ui.spinner();
                    ui.label("IP bilgileri sorgulanıyor…");
                }
            });
        if !open {
            self.intel = None;
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(route) = &self.route {
            route.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl eframe::App for Watch {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.icons.refresh(ctx);
        if let Some(snapshot) = self.pending.lock().unwrap().take() {
            self.snapshot = snapshot;
            self.waiting = false;
            if self
                .selected
                .is_some_and(|key| !self.snapshot.connections.iter().any(|c| c.key == key))
            {
                self.selected = None;
            }
        }
        if let Some(route) = &mut self.route
            && route.finish.is_none()
        {
            loop {
                match route.receiver.try_recv() {
                    Ok(Event::Hop(hop)) => route.hops.push(hop),
                    Ok(Event::Done(finish)) => {
                        route.finish = Some(finish);
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        route.finish = Some(Finish::Failed(31)); // ERROR_GEN_FAILURE
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let colors = theme::colors(ui.ctx());
        Frame::central_panel(ui.style())
            .fill(colors.background)
            .inner_margin(0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if header(ui, self.snapshot.connections.len(), !self.waiting) {
                    native::caption(frame, theme::colors(ui.ctx()));
                }
                Frame::new().inner_margin(13).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(11.0, 4.0);
                    let width = ui.available_width() - 11.0;
                    let height = ui.available_height();
                    ui.horizontal_top(|ui| {
                        for (left, width) in [(true, width * 0.59), (false, width * 0.41)] {
                            ui.allocate_ui_with_layout(
                                vec2(width, height),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(width);
                                    panel(colors).show(ui, |ui| {
                                        let inner = vec2(width - 34.0, height - 34.0);
                                        ui.set_min_size(inner);
                                        ui.set_max_size(inner);
                                        if left {
                                            self.connections(ui);
                                        } else {
                                            self.routes(ui);
                                        }
                                    });
                                },
                            );
                        }
                    });
                });
            });
        self.intel_window(ui);
    }
}

fn header(ui: &mut Ui, connections: usize, ready: bool) -> bool {
    brand_bar(ui, connections, ready);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 30.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.menu_button("Görünüm", |ui| theme::picker(ui, THEME_APP, "Tema"))
                .inner
                == Some(true)
        },
    )
    .inner
}

fn route_button(text: &str, primary: bool, colors: theme::Colors) -> Button<'_> {
    Button::new(RichText::new(text).font(bold(14.0)).color(if primary {
        colors.on_accent
    } else {
        colors.text
    }))
    .min_size(vec2(if primary { 97.0 } else { 85.0 }, 34.0))
    .corner_radius(6)
    .fill(if primary {
        colors.accent
    } else {
        colors.surface
    })
    .stroke(if primary {
        Stroke::NONE
    } else {
        Stroke::new(1.0, colors.border)
    })
}

fn table_cells(rect: Rect) -> [Rect; 6] {
    let bounds = [0.0, 0.243, 0.472, 0.554, 0.688, 0.842, 1.0];
    std::array::from_fn(|i| {
        Rect::from_min_max(
            pos2(rect.left() + rect.width() * bounds[i] + 7.0, rect.top()),
            pos2(
                rect.left() + rect.width() * bounds[i + 1] - 6.0,
                rect.bottom(),
            ),
        )
    })
}

fn table_heading(ui: &mut Ui, width: f32) {
    let colors = theme::colors(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, 37.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 5.0, colors.surface);
    p.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, colors.border),
    );
    for (index, (cell, label)) in table_cells(rect)
        .into_iter()
        .zip(["Uygulama", "Uzak IP", "Port", "Durum", "İndirme", "Yükleme"])
        .enumerate()
    {
        let right = index >= 4;
        if right {
            let width = p
                .layout_no_wrap(label.into(), bold(14.0), colors.text)
                .size()
                .x;
            arrow(
                p,
                cell.right_center() - vec2(width + 11.0, 0.0),
                index == 5,
                colors.muted,
            );
        }
        cell_text(p, cell, label, bold(14.0), colors.text, right);
    }
}

fn connection_row(
    ui: &mut Ui,
    width: f32,
    c: &Connection,
    selected: bool,
    icon: Option<egui::TextureId>,
) -> egui::Response {
    let colors = theme::colors(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let response = ui.interact(rect, ui.make_persistent_id(c.key), Sense::click());
    let p = ui.painter();
    if selected || response.hovered() {
        p.rect_filled(
            rect,
            4.0,
            if selected {
                colors.selected
            } else {
                colors.surface
            },
        );
        if selected {
            p.rect_filled(
                Rect::from_min_size(rect.left_top() + vec2(0.0, 3.0), vec2(3.0, ROW - 6.0)),
                1.0,
                colors.accent_text,
            );
        }
    }
    p.hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, colors.border),
    );

    let state = monitor::state_label(c.state);
    let color = match state {
        "Bağlı" => colors.success,
        "Kuruluyor" => colors.warning,
        _ => colors.muted,
    };
    let rate = c.transfer.rate();
    let labels = [
        c.process.name.clone(),
        address(c.key.remote),
        c.key.remote.port().to_string(),
        state.to_owned(),
        rate.map_or("—".into(), |r| quantity(r.down, true)),
        rate.map_or("—".into(), |r| quantity(r.up, true)),
    ];
    for (i, mut cell) in table_cells(rect).into_iter().enumerate() {
        if i == 0 {
            let icon_rect =
                Rect::from_center_size(cell.left_center() + vec2(11.0, 0.0), vec2(22.0, 22.0));
            if let Some(icon) = icon {
                p.image(
                    icon,
                    icon_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                let fallback = icon_rect.shrink(2.0);
                p.rect(
                    fallback,
                    4.0,
                    colors.surface,
                    Stroke::new(1.0, colors.border),
                    StrokeKind::Inside,
                );
                p.hline(
                    fallback.x_range().shrink(4.0),
                    fallback.top() + 6.0,
                    Stroke::new(1.0, colors.muted),
                );
            }
            cell.min.x += 29.0;
        } else if i == 3 {
            p.circle_filled(cell.left_center() + vec2(4.0, 0.0), 3.8, color);
            cell.min.x += 13.0;
        }
        let font = if i == 0 && selected {
            bold(15.0)
        } else {
            FontId::proportional(15.0)
        };
        let color = if i == 3 || (i >= 4 && rate.is_none()) {
            colors.muted
        } else {
            colors.text
        };
        cell_text(p, cell, &labels[i], font, color, i >= 4);
    }
    let mut detail = format!(
        "{} · PID {}\n{} → {}",
        c.process.name, c.key.pid, c.key.local, c.key.remote
    );
    if let Some(code) = c.traffic_error {
        detail.push_str(&format!("\nTrafik verisi kullanılamıyor (Windows {code})."));
    } else if c.transfer.has_counters() && rate.is_none() {
        detail.push_str("\nHız için bir sonraki örnek bekleniyor.");
    }
    response.on_hover_text(detail)
}

fn hop_row(ui: &mut Ui, width: f32, hop: &Hop, last: bool, reached: bool) -> Option<IpAddr> {
    let colors = theme::colors(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, HOP), Sense::hover());
    let p = ui.painter();
    let center = rect.left_center() + vec2(18.0, 0.0);
    let missing = hop.reply.is_none();
    let color = if reached {
        colors.success
    } else if missing {
        colors.warning
    } else {
        colors.muted
    };
    if hop.number != 1 {
        p.line_segment(
            [pos2(center.x, rect.top()), center],
            Stroke::new(2.0, colors.border),
        );
    }
    if !last {
        p.line_segment(
            [center, pos2(center.x, rect.bottom())],
            Stroke::new(2.0, colors.border),
        );
    }
    p.circle(
        center,
        13.0,
        if reached {
            theme::mix(colors.surface, colors.success, 18)
        } else {
            colors.surface
        },
        Stroke::new(1.5, color),
    );
    p.text(
        center,
        Align2::CENTER_CENTER,
        hop.number.to_string(),
        bold(13.0),
        if missing { colors.warning } else { colors.text },
    );
    p.hline(
        egui::Rangef::new(rect.left() + 42.0, rect.right()),
        rect.bottom(),
        Stroke::new(1.0, colors.border),
    );
    let ip_rect = Rect::from_min_max(
        pos2(rect.left() + 45.0, rect.top()),
        pos2(rect.right() - 67.0, rect.bottom()),
    );
    let time_rect = Rect::from_min_max(pos2(rect.right() - 66.0, rect.top()), rect.right_bottom());
    let mut inspect = None;
    if let Some((ip, ms)) = hop.reply {
        let ip = ip.to_canonical();
        let mut color = if reached { colors.success } else { colors.text };
        if is_public_candidate(ip) {
            let ip_width = p
                .layout_no_wrap(ip.to_string(), bold(16.0), colors.text)
                .size()
                .x;
            let area = Rect::from_center_size(
                ip_rect.left_center() + vec2(ip_width.min(ip_rect.width()) / 2.0, 0.0),
                vec2(ip_width.min(ip_rect.width()), 28.0),
            );
            let response = ui
                .interact(
                    area,
                    ui.make_persistent_id(("inspect hop", hop.number)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("IP’yi incele");
            if response.hovered() {
                color = colors.secondary_text;
            }
            if response.clicked() {
                inspect = Some(ip);
            }
        }
        cell_text(p, ip_rect, &ip.to_string(), bold(16.0), color, false);
        cell_text(
            p,
            time_rect,
            &rtt(ms),
            FontId::proportional(15.0),
            colors.text,
            true,
        );
    } else {
        cell_text(
            p,
            ip_rect,
            "Yanıt yok",
            FontId::proportional(15.0),
            colors.warning,
            false,
        );
        cell_text(
            p,
            time_rect,
            "—",
            FontId::proportional(15.0),
            colors.muted,
            true,
        );
    }
    inspect
}

fn route_status(ui: &mut Ui, route: Option<&Trace>) {
    let colors = theme::colors(ui.ctx());
    let (title, detail, color) = match route {
        Some(trace) => match trace.finish {
            Some(Finish::Reached) => {
                let result = trace.hops.last().and_then(|hop| hop.reply).map_or_else(
                    || address(trace.target),
                    |(ip, ms)| format!("{}  ·  {}", ip.to_canonical(), rtt(ms)),
                );
                ("Hedefe ulaşıldı".to_owned(), result, colors.success)
            }
            Some(Finish::Limit) => (
                "Hedef yanıt vermedi".to_owned(),
                format!("{} adım ölçüldü", route::MAX_HOPS),
                colors.warning,
            ),
            Some(Finish::Cancelled) => (
                "Ölçüm durduruldu".to_owned(),
                format!("{} adım ölçüldü", trace.hops.len()),
                colors.muted,
            ),
            Some(Finish::Failed(code)) => (
                "Ölçüm tamamlanamadı".to_owned(),
                format!("Windows hata kodu {code}"),
                colors.error,
            ),
            None => (
                "Rota ölçülüyor…".to_owned(),
                format!("{} adım ölçüldü", trace.hops.len()),
                colors.accent_text,
            ),
        },
        None => (
            "Ölçüme hazır".to_owned(),
            "Yanıt veren hop’lar bu panelde görünür.".to_owned(),
            colors.muted,
        ),
    };
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
    let p = ui.painter();
    p.rect(
        rect,
        7.0,
        colors.field,
        Stroke::new(1.0, colors.border),
        StrokeKind::Inside,
    );
    p.circle_filled(rect.left_top() + vec2(16.0, 19.0), 4.0, color);
    p.text(
        rect.left_top() + vec2(29.0, 6.0),
        Align2::LEFT_TOP,
        title,
        bold(16.0),
        color,
    );
    cell_text(
        p,
        Rect::from_min_max(
            rect.left_top() + vec2(29.0, 28.0),
            rect.right_bottom() - vec2(7.0, 4.0),
        ),
        &detail,
        FontId::proportional(14.0),
        colors.muted,
        false,
    );
}

fn rtt(ms: u32) -> String {
    if ms == 0 {
        "<1 ms".into()
    } else {
        format!("{ms} ms")
    }
}

fn address(socket: SocketAddr) -> String {
    match socket {
        SocketAddr::V6(v6) if v6.scope_id() != 0 => format!("{}%{}", v6.ip(), v6.scope_id()),
        _ => socket.ip().to_string(),
    }
}

fn is_public_candidate(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [first, second, _, _] = ip.octets();
            (1..224).contains(&first)
                && !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_documentation()
                && !(first == 100 && (64..=127).contains(&second))
                && !(first == 198 && matches!(second, 18 | 19))
        }
        IpAddr::V6(ip) => {
            (ip.segments()[0] & 0xe000) == 0x2000
                && !(ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8)
        }
    }
}

fn lookup(ip: IpAddr) -> Result<String, String> {
    let app = std::env::current_exe().map_err(|error| error.to_string())?;
    let tool = app.with_file_name("ipintel.exe");
    if !tool.is_file() {
        return Err("Barracuda IP Intelligence paketindeki ipintel.exe dosyasını bu uygulamayla aynı klasöre koy.".into());
    }
    let output = Command::new(&tool)
        .arg(ip.to_string())
        .arg("--print-json")
        .current_dir(tool.parent().expect("the executable path has a parent"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("IP Intelligence başlatılamadı · {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "IP Intelligence hata bildirdi · {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_leaves_the_body_visible_in_normal_and_maximized_windows() {
        let ctx = egui::Context::default();
        setup(&ctx);
        for size in [vec2(990.0, 580.0), vec2(1920.0, 1120.0)] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let before = ui.available_height();
                header(ui, 35, true);
                let used = before - ui.available_height();
                assert!(used <= 100.0, "Header consumed {used} points at {size:?}");
            });
            output.textures_delta.clear();
        }
    }
}
