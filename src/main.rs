mod app;
mod editor_buffer;
mod expansion;
mod metrics;
mod storage;

use std::time::Instant;

use app::LexwrightApp;

fn main() -> eframe::Result<()> {
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
