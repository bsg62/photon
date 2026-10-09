// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use photon_ui::{
    app::App,
    args::{Args, USAGE},
    dirs::IDENTIFIER,
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let Some(dirs) = args.dirs() else {
        eprintln!("photon could not find a directory to keep its library in");
        return ExitCode::FAILURE;
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("photon")
            // What a Wayland compositor and a taskbar know the window by.
            .with_app_id(IDENTIFIER)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_fullscreen(args.fullscreen),
        ..Default::default()
    };
    let run = eframe::run_native(
        "photon",
        options,
        Box::new(move |cc| {
            tracing::info!(
                adapter = ?cc.wgpu_render_state.as_ref().map(|state| state.adapter.get_info()),
                "started"
            );
            match App::new(cc, dirs, ::dirs::picture_dir()) {
                Ok(app) => Ok(Box::new(app) as Box<dyn eframe::App>),
                Err(message) => Err(message.into()),
            }
        }),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "photon could not start");
            eprintln!("photon could not start: {err}");
            ExitCode::FAILURE
        }
    }
}
