mod analysis;
mod app;
mod cli;
mod document;
mod editor_buffer;
mod expansion;
mod harper;
mod jitter;
mod metrics;
mod navigation;
mod overlay;
mod probe;
mod settings;
mod storage;
mod workspace;

use std::time::Instant;

use app::LexwrightApp;
use cli::Command;

fn main() -> eframe::Result<()> {
    let launch = match cli::parse_args(std::env::args().skip(1)) {
        Ok(Command::Launch(options)) => options,
        Ok(Command::LatencyProbe) => {
            probe::run();
            return Ok(());
        }
        Ok(Command::Help) => {
            print!("{}", cli::help_text());
            return Ok(());
        }
        Err(error) => {
            eprintln!("lexwright: {error}\n\n{}", cli::help_text());
            std::process::exit(2);
        }
    };

    // Hyprland must float the window before mapping, not after tiling it.
    match overlay::maybe_launch_via_hyprland(&launch) {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(error) => {
            eprintln!("lexwright: {error}");
            std::process::exit(1);
        }
    }

    let process_started = Instant::now();
    let mut viewport = eframe::egui::ViewportBuilder::default();
    if launch.overlay {
        viewport = viewport
            .with_inner_size([860.0, 540.0])
            .with_min_inner_size([460.0, 310.0])
            .with_app_id("lexwright-scratch-overlay")
            .with_always_on_top();
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport,
        ..Default::default()
    };

    let window_title = if launch.overlay {
        "Lexwright — Scratch Overlay"
    } else if launch.scratch {
        "Lexwright — Scratch"
    } else {
        "Lexwright"
    };

    eframe::run_native(
        window_title,
        options,
        Box::new(move |cc| {
            Ok(Box::new(LexwrightApp::new(
                cc,
                process_started,
                launch.clone(),
            )))
        }),
    )
}
