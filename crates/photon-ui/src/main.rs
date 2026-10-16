// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use photon_ui::{
    app::App,
    args::{Args, USAGE},
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
        // Said where it can be read with no window, as a failed start is below: a
        // release build on Windows has no console to print it in.
        let said = "photon could not find a directory to keep its library in.";
        eprintln!("{said}");
        photon_ui::platform::show_error(said);
        return ExitCode::FAILURE;
    };

    // Dates and counts in the system's language and order, from the first frame. Here,
    // where the window is the program's, and not in `App::new`, which every test of the
    // whole application calls too: a test's words are English whatever the machine's.
    photon_ui::locale::set(photon_ui::locale::Locale::system());

    let probe = args.probe.clone();
    let fullscreen = args.fullscreen;
    let options = photon_ui::window::native_options(&args, &dirs);
    let run = eframe::run_native(
        "photon",
        options,
        Box::new(move |cc| {
            match App::new(cc, dirs, ::dirs::picture_dir()) {
                Ok(app) => {
                    tracing::info!(adapter = ?app.adapter(), "started");
                    // As `window::native_options` keeps nothing of a fullscreen run's
                    // window, the application puts nothing right in what was kept.
                    let app = if fullscreen {
                        app.without_window_kept()
                    } else {
                        app
                    };
                    let app = match probe {
                        // When the harness started this process, for the launch time; a
                        // run by hand has none and the report says so.
                        Some(out) if warm_up() => app.with_warm_up(out, started_epoch_ms()),
                        Some(out) => app.with_probe(out, started_epoch_ms()),
                        None => app,
                    };
                    Ok(Box::new(app) as Box<dyn eframe::App>)
                }
                Err(message) => Err(message.into()),
            }
        }),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        // Said where it can be read with no window: in the system's message box, which on
        // Linux needs `zenity`, and on the standard error and in the log, which need
        // nothing.
        Err(err) => {
            tracing::error!(%err, "photon could not start");
            let said = photon_ui::platform::startup_failed(&err);
            eprintln!("{said}");
            photon_ui::platform::show_error(&said);
            ExitCode::FAILURE
        }
    }
}

/// `PHOTON_PROBE_T0`: the epoch time, in milliseconds, at which the gate's harness started
/// this process.
fn started_epoch_ms() -> Option<f64> {
    std::env::var("PHOTON_PROBE_T0").ok()?.parse().ok()
}

/// `PHOTON_PROBE_WARMUP`: the gate's harness is launching this once to be thrown away,
/// before the launch it measures.
fn warm_up() -> bool {
    std::env::var_os("PHOTON_PROBE_WARMUP").is_some()
}
