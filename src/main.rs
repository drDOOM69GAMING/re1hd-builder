#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use re1hd_builder::app;

fn load_icon() -> Option<eframe::egui::IconData> {
    let bytes = include_bytes!("../assets/icon.ico");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Ico).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(eframe::egui::IconData {
        rgba: rgba.into_raw().into(),
        width,
        height,
    })
}

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("RE1HD Builder")
        .with_inner_size([980.0, 900.0])
        .with_min_inner_size([820.0, 700.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "RE1HD Builder",
        options,
        Box::new(|cc| Ok(Box::new(app::AppState::new(cc)))),
    )
}