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

    let process_started = Instant::now();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let window_title = if launch.scratch {
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
