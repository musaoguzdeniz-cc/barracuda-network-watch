#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod monitor;
mod native;
mod route;
mod ui;
mod visuals;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1320.0, 760.0])
            .with_min_inner_size([990.0, 580.0])
            .with_icon(native::app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Barracuda Network Watch",
        options,
        Box::new(|cc| Ok(Box::new(ui::Watch::new(cc)))),
    )
}
