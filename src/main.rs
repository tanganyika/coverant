// no console window behind the program on Windows
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod coords;
mod coverage;
mod itm;
mod map;
mod model;
mod terrain;
mod theme;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut shot = None;
    let mut tab = None;
    let mut plot = false;
    let mut save = false;
    let mut popup: Option<(f64, f64)> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => shot = args.next().map(PathBuf::from),
            "--tab" => tab = args.next().and_then(|t| app::Tab::from_name(&t)),
            "--plot" => plot = true,
            "--save" => save = true,
            "--popup" => popup = args.next().and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))),
            "--help" | "-h" => {
                println!("coverant [--tab sites|antenna|coverage] [--plot] [--screenshot out.png]");
                return Ok(());
            }
            _ => {}
        }
    }
    let glow = std::env::var("COVERANT_RENDERER").map(|r| r.eq_ignore_ascii_case("glow")).unwrap_or(false);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([800.0, 560.0])
            .with_title("Coverant")
            .with_app_id("coverant")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/coverant.png")).unwrap_or_default()),
        renderer: if glow { eframe::Renderer::Glow } else { eframe::Renderer::Wgpu },
        ..Default::default()
    };
    let r = eframe::run_native(
        "Coverant",
        options.clone(),
        Box::new(move |cc| {
            let mut a = app::App::new(cc, tab, shot.clone());
            if save {
                a.save_on_start();
            }
            if plot {
                a.plot_on_start();
            }
            if let Some((lat, lon)) = popup {
                a.popup_at(lat, lon);
            }
            Ok(Box::new(a))
        }),
    );
    if r.is_err() && !glow {
        // wgpu couldn't start (old driver, virtual machine): try plain OpenGL
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(st) = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("COVERANT_RENDERER", "glow").status() {
                std::process::exit(st.code().unwrap_or(1));
            }
        }
    }
    r
}
