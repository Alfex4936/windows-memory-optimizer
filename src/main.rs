#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod memory;
mod optimization;
mod preferences;
mod sort;
mod startup;
mod ui;
mod window;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if let Some(enabled) = args.iter().find_map(|a| match a.as_str() {
        "--enable-startup" => Some(true),
        "--disable-startup" => Some(false),
        _ => None,
    }) {
        if let Err(error) = preferences::set_launch_at_login(enabled) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let (mut preferences, load_error) = match preferences::load() {
        Ok(p) => (p, None),
        Err(e) => (Default::default(), Some(e)),
    };
    if args.iter().any(|a| a == "--compact") {
        preferences.compact = true;
    }
    if args.iter().any(|a| a == "--expanded") {
        preferences.compact = false;
    }
    let _ = memory::protected_foreground_pid();
    let widget = ui::starts_as_widget();
    let small_window = args.iter().any(|a| a == "--small-window");
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Embedded Memori icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(icon)
            .with_inner_size(if widget {
                ui::widget_size(preferences.compact).into()
            } else if small_window {
                [880.0, 650.0]
            } else {
                [1120.0, 790.0]
            })
            .with_min_inner_size(if widget {
                ui::widget_size(preferences.compact).into()
            } else {
                [880.0, 650.0]
            })
            .with_decorations(!widget)
            .with_transparent(true)
            .with_resizable(!widget)
            .with_window_level(if widget && preferences.pinned {
                eframe::egui::WindowLevel::AlwaysOnTop
            } else {
                eframe::egui::WindowLevel::Normal
            })
            .with_title("Memori · 메모리와 시작 프로그램"),
        ..Default::default()
    };
    eframe::run_native(
        "Memori",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, preferences, load_error)))),
    )
}
