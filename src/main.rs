#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod monitor;
mod route;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 760.0])
            .with_min_inner_size([970.0, 580.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Barracuda Network Watch",
        options,
        Box::new(|cc| Ok(Box::new(ui::Watch::new(cc)))),
    )
}
