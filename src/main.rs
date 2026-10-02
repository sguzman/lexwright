mod app;
mod storage;

use app::LexwrightApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Lexwright",
        options,
        Box::new(|cc| Ok(Box::new(LexwrightApp::new(cc)))),
    )
}
