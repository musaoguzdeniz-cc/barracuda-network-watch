use crate::monitor::{self, Connection, Key, Monitor, Snapshot, Transfer};
use crate::route::{self, Event, Finish, Hop};
use eframe::egui::{
    self, Align, Align2, Button, Color32, FontId, Frame, Layout, Margin, Rect, RichText, Sense,
    Stroke, Ui, Vec2, pos2, vec2,
};
use std::net::{IpAddr, SocketAddr};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const BG: Color32 = Color32::from_rgb(19, 20, 23);
const PANEL: Color32 = Color32::from_rgb(26, 27, 31);
const FIELD: Color32 = Color32::from_rgb(21, 22, 25);
const BORDER: Color32 = Color32::from_rgb(56, 58, 65);
const TEXT: Color32 = Color32::from_rgb(242, 241, 244);
const MUTED: Color32 = Color32::from_rgb(175, 177, 187);
const CRIMSON: Color32 = Color32::from_rgb(190, 21, 62);
const SELECTED: Color32 = Color32::from_rgb(103, 23, 44);
const GREEN: Color32 = Color32::from_rgb(107, 233, 158);
const AMBER: Color32 = Color32::from_rgb(242, 189, 91);
const BLUE: Color32 = Color32::from_rgb(81, 177, 242);

pub struct Watch {
    pending: Arc<Mutex<Option<Snapshot>>>,
    stop: Arc<AtomicBool>,
    snapshot: Snapshot,
    waiting: bool,
    search: String,
    selected: Option<Key>,
    route: Option<Trace>,
    intel: Option<Intel>,
}

struct Trace {
    local: SocketAddr,
    target: SocketAddr,
    receiver: mpsc::Receiver<Event>,
    cancel: Arc<AtomicBool>,
    hops: Vec<Hop>,
    finish: Option<Finish>,
}

struct Intel {
    ip: IpAddr,
    receiver: mpsc::Receiver<Result<String, String>>,
    result: Option<Result<String, String>>,
}

impl Watch {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT);
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = FIELD;
        visuals.faint_bg_color = FIELD;
        visuals.selection.bg_fill = SELECTED;
        visuals.widgets.inactive.bg_fill = FIELD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.hovered.bg_fill = BORDER;
        cc.egui_ctx.set_visuals(visuals);

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
        self.route = Some(Trace {
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
        ui.heading(RichText::new("Canlı bağlantılar").size(26.0).strong());
        ui.label(
            RichText::new("Uygulamaların TCP bağlantıları ve anlık veri trafiği").color(MUTED),
        );
        ui.add_space(10.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Uygulama, IP, port veya PID ara")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);

        if !self.snapshot.table_errors.is_empty() {
            ui.colored_label(
                AMBER,
                format!(
                    "Bağlantı tablosu okunamadı · {}",
                    self.snapshot.table_errors.join(", ")
                ),
            );
        }
        if self
            .snapshot
            .connections
            .iter()
            .any(|c| c.traffic_error == Some(5))
        {
            ui.colored_label(
                AMBER,
                "Trafik ölçümü için uygulamayı yönetici olarak çalıştır.",
            );
        }

        let mut rows = 0;
        table_row(
            ui,
            &["Uygulama", "Uzak IP", "Port", "Durum", "İndirme", "Yükleme"],
            None,
        );
        let query = self.search.trim().to_lowercase();
        let height = (ui.available_height() - 195.0).max(85.0);
        egui::ScrollArea::vertical()
            .id_salt("connections")
            .max_height(height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for connection in &self.snapshot.connections {
                    let key = connection.key;
                    let ip = address(key.remote);
                    let port = key.remote.port().to_string();
                    if !query.is_empty()
                        && !connection.process.to_lowercase().contains(&query)
                        && !ip.contains(&query)
                        && !port.contains(&query)
                        && !key.pid.to_string().contains(&query)
                    {
                        continue;
                    }
                    rows += 1;
                    let rate = connection.transfer.rate();
                    let down = rate.map_or("—".into(), |value| quantity(value.down, true));
                    let up = rate.map_or("—".into(), |value| quantity(value.up, true));
                    let response = table_row(
                        ui,
                        &[
                            &connection.process,
                            &ip,
                            &port,
                            monitor::state_label(connection.state),
                            &down,
                            &up,
                        ],
                        Some(self.selected == Some(key)),
                    );
                    if response.clicked() {
                        self.selected = Some(key);
                    }
                    let mut detail = format!(
                        "{} · PID {}\n{} → {}",
                        connection.process, key.pid, key.local, key.remote
                    );
                    if let Some(code) = connection.traffic_error {
                        detail
                            .push_str(&format!("\nTrafik verisi kullanılamıyor (Windows {code})."));
                    } else if connection.transfer.has_counters() && rate.is_none() {
                        detail.push_str("\nHız için bir sonraki örnek bekleniyor.");
                    }
                    response.on_hover_text(detail);
                }
                if rows == 0 {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(if self.waiting {
                            "Bağlantılar okunuyor…"
                        } else {
                            "Gösterilecek bağlantı yok."
                        })
                        .color(MUTED),
                    );
                }
            });
        ui.add_space(10.0);
        if let Some(connection) = self.selected_connection() {
            transfer_panel(ui, connection);
        } else {
            panel().show(ui, |ui| {
                ui.heading("Seçili bağlantının trafiği");
                ui.label(
                    RichText::new("Hız geçmişi ve toplam veri için bir bağlantı seç.").color(MUTED),
                );
            });
        }
    }

    fn selected_connection(&self) -> Option<&Connection> {
        self.snapshot
            .connections
            .iter()
            .find(|c| Some(c.key) == self.selected)
    }

    fn routes(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("Hedefe ölçülen rota").size(26.0).strong());
        ui.label(RichText::new("Ayrı ICMP ölçümü · Gidiş-dönüş süresi").color(MUTED));
        ui.add_space(12.0);
        let busy = self
            .route
            .as_ref()
            .is_some_and(|route| route.finish.is_none());
        let selected = self.selected_connection().map(|c| c.key);
        let shown = self.route.as_ref().filter(|route| {
            busy || selected
                .is_none_or(|key| route.local == key.local && route.target == key.remote)
        });
        let target = shown
            .map(|route| route.target)
            .or(selected.map(|key| key.remote));
        ui.horizontal_wrapped(|ui| {
            if let Some(target) = target {
                ui.label(
                    RichText::new(address(target))
                        .monospace()
                        .strong()
                        .size(16.0),
                );
                let ip = target.ip().to_canonical();
                if ui
                    .add_enabled(is_public_candidate(ip), Button::new("IP’yi incele"))
                    .on_hover_text("Barracuda IP Intelligence ile aç")
                    .clicked()
                {
                    self.start_intel(ip, ui.ctx());
                }
            }
            if busy {
                if ui.button("Ölçümü durdur").clicked() {
                    self.route
                        .as_ref()
                        .unwrap()
                        .cancel
                        .store(true, Ordering::Relaxed);
                }
            } else if let Some(key) = selected
                && ui
                    .add(
                        Button::new(RichText::new("Rotayı ölç").color(Color32::WHITE))
                            .fill(CRIMSON),
                    )
                    .clicked()
            {
                self.start_trace(key, ui.ctx());
            }
        });
        ui.separator();
        let Some(route) = self.route.as_ref().filter(|route| {
            busy || selected
                .is_none_or(|key| route.local == key.local && route.target == key.remote)
        }) else {
            ui.label(
                RichText::new("Bir TCP bağlantısı seçip Rotayı ölç düğmesine bas.").color(MUTED),
            );
            return;
        };

        let (status, color) = match route.finish {
            Some(Finish::Reached) => ("Hedefe ulaşıldı".into(), GREEN),
            Some(Finish::Limit) => (
                format!("{} adımda hedef yanıt vermedi", route::MAX_HOPS),
                AMBER,
            ),
            Some(Finish::Cancelled) => ("Ölçüm durduruldu".into(), MUTED),
            Some(Finish::Failed(code)) => (format!("Ölçüm tamamlanamadı · Windows {code}"), AMBER),
            None => ("Rota ölçülüyor…".into(), MUTED),
        };
        let mut inspect = None;
        egui::ScrollArea::vertical()
            .id_salt("hops")
            .max_height((ui.available_height() - 65.0).max(65.0))
            .show(ui, |ui| {
                for hop in &route.hops {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [29.0, 28.0],
                            egui::Label::new(
                                RichText::new(hop.number.to_string())
                                    .monospace()
                                    .color(MUTED),
                            ),
                        );
                        if let Some((ip, ms)) = hop.reply {
                            let ip = ip.to_canonical();
                            if is_public_candidate(ip) {
                                if ui
                                    .link(RichText::new(ip.to_string()).monospace())
                                    .on_hover_text("IP’yi incele")
                                    .clicked()
                                {
                                    inspect = Some(ip);
                                }
                            } else {
                                ui.label(RichText::new(ip.to_string()).monospace());
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(if ms == 0 {
                                    "<1 ms".to_owned()
                                } else {
                                    format!("{ms} ms")
                                });
                            });
                        } else {
                            ui.colored_label(AMBER, "Yanıt yok");
                        }
                    });
                    ui.separator();
                }
            });
        ui.colored_label(color, RichText::new(status).strong());
        if let Some(ip) = inspect {
            self.start_intel(ip, ui.ctx());
        }
    }

    fn intel_window(&mut self, ui: &mut Ui) {
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
                    ui.colored_label(AMBER, message.as_str());
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
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        Frame::central_panel(ui.style())
            .fill(BG)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    ui.painter().rect_filled(mark.shrink(2.0), 4.0, CRIMSON);
                    ui.label(RichText::new("Barracuda Network Watch").size(20.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} TCP bağlantısı",
                                self.snapshot.connections.len()
                            ))
                            .color(MUTED),
                        );
                        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.5, GREEN);
                    });
                });
                ui.add_space(9.0);
                let width = ui.available_width();
                let height = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for (index, width) in [(0, (width - 10.0) * 0.61), (1, (width - 10.0) * 0.39)] {
                        let size = vec2(width, height);
                        ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                            ui.set_width(width);
                            panel().show(ui, |ui| {
                                let inner = vec2(width - 30.0, (height - 30.0).max(100.0));
                                ui.set_min_size(inner);
                                ui.set_max_size(inner);
                                if index == 0 {
                                    self.connections(ui);
                                } else {
                                    self.routes(ui);
                                }
                            });
                        });
                    }
                });
            });
        self.intel_window(ui);
    }
}

fn panel() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(9)
        .inner_margin(Margin::same(14))
}

fn table_row(ui: &mut Ui, labels: &[&str; 6], selected: Option<bool>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 33.0),
        if selected.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = match selected {
        None => FIELD,
        Some(true) => SELECTED,
        Some(false) if response.hovered() => BORDER,
        Some(false) => PANEL,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, fill);
    painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, BORDER));
    let shares = [0.215, 0.258, 0.077, 0.12, 0.165, 0.165];
    let mut x = rect.left();
    for (index, (label, share)) in labels.iter().zip(shares).enumerate() {
        let w = rect.width() * share;
        let cell = Rect::from_min_max(pos2(x + 5.0, rect.top()), pos2(x + w - 3.0, rect.bottom()));
        let font = if selected.is_some() && matches!(index, 1 | 2 | 4 | 5) {
            FontId::monospace(12.0)
        } else {
            FontId::proportional(13.0)
        };
        let color = if index == 3 && *label == "Bağlı" {
            GREEN
        } else {
            TEXT
        };
        let painter = painter.with_clip_rect(cell);
        let right = index >= 4;
        painter.text(
            if right {
                cell.right_center()
            } else {
                cell.left_center()
            },
            if right {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            label,
            font,
            color,
        );
        x += w;
    }
    response
}

fn transfer_panel(ui: &mut Ui, connection: &Connection) {
    panel().fill(FIELD).show(ui, |ui| {
        ui.heading("Seçili bağlantının trafiği");
        ui.label(
            RichText::new(format!(
                "{} · {}",
                connection.process, connection.key.remote
            ))
            .monospace()
            .color(MUTED),
        );
        ui.add_space(5.0);
        let transfer = &connection.transfer;
        let measured = transfer.has_counters();
        ui.label(
            RichText::new("İzleme başlangıcından beri ölçülen")
                .color(MUTED)
                .size(12.0),
        );
        ui.horizontal_wrapped(|ui| {
            let down = if measured {
                quantity(transfer.totals.down as f64, false)
            } else {
                "—".into()
            };
            let up = if measured {
                quantity(transfer.totals.up as f64, false)
            } else {
                "—".into()
            };
            ui.colored_label(
                CRIMSON.gamma_multiply(1.55),
                RichText::new(format!("İndirilen {down}"))
                    .strong()
                    .size(18.0),
            );
            ui.colored_label(
                BLUE,
                RichText::new(format!("Yüklenen {up}")).strong().size(18.0),
            );
        })
        .response
        .on_hover_text("Toplam indirilen ve yüklenen veri");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Son 60 saniye").color(MUTED).size(12.0));
            ui.colored_label(CRIMSON.gamma_multiply(1.55), "İndirme");
            ui.colored_label(BLUE, "Yükleme");
        });
        chart(ui, transfer);
    });
}

fn chart(ui: &mut Ui, transfer: &Transfer) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 51.0), Sense::hover());
    let Some(latest) = transfer.history.back() else {
        return;
    };
    let max = transfer
        .history
        .iter()
        .filter_map(|sample| sample.rate)
        .map(|rate| rate.down.max(rate.up))
        .fold(0.0_f64, f64::max)
        .max(1000.0);
    let graph = Rect::from_min_max(rect.min + vec2(74.0, 5.0), rect.max - vec2(2.0, 5.0));
    let painter = ui.painter();
    painter.text(
        graph.left_top() - vec2(5.0, 0.0),
        Align2::RIGHT_TOP,
        quantity(max, true),
        FontId::proportional(10.0),
        MUTED,
    );
    painter.text(
        graph.left_bottom() - vec2(5.0, 0.0),
        Align2::RIGHT_BOTTOM,
        "0",
        FontId::proportional(10.0),
        MUTED,
    );
    for y in [graph.top(), graph.center().y, graph.bottom()] {
        painter.hline(graph.x_range(), y, Stroke::new(1.0, BORDER));
    }
    for (upload, color) in [(false, CRIMSON), (true, BLUE)] {
        let mut last = None;
        for sample in &transfer.history {
            let point = sample.rate.map(|rate| {
                let seconds = (latest.time - sample.time).as_secs_f32();
                let x = graph.right() - (seconds / 60.0).min(1.0) * graph.width();
                let y = if upload { rate.up } else { rate.down };
                pos2(x, graph.bottom() - (y / max) as f32 * graph.height())
            });
            if let Some(point) = point {
                if let Some(previous) = last {
                    painter.line_segment([previous, point], Stroke::new(2.0, color));
                }
                painter.circle_filled(point, 1.6, color);
            }
            last = point;
        }
    }
}

fn quantity(value: f64, rate: bool) -> String {
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
