mod analysis;
mod app;
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

fn main() -> eframe::Result<()> {
    if std::env::args().any(|argument| argument == "--latency-probe") {
        probe::run();
        return Ok(());
    }

    let process_started = Instant::now();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Lexwright",
        options,
        Box::new(move |cc| Ok(Box::new(LexwrightApp::new(cc, process_started)))),
    )
}
