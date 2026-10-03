use std::{
    env,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
};

pub struct JitterRecorder {
    tx: Sender<String>,
    path: PathBuf,
}

pub struct JitterFrame {
    pub elapsed_us: u128,
    pub revision: u64,
    pub changed: bool,
    pub editor_x: f32,
    pub editor_y: f32,
    pub editor_w: f32,
    pub editor_h: f32,
    pub galley_x: f32,
    pub galley_y: f32,
    pub galley_w: f32,
    pub galley_h: f32,
    pub wrap_w: f32,
    pub rows: usize,
    pub cursor_index: usize,
    pub cursor_row: usize,
    pub cursor_column: usize,
    pub queued_revision: u64,
    pub saved_revision: u64,
    pub analysis_revision: Option<u64>,
    pub harper_revision: Option<u64>,
    pub expansion_hits: u64,
}

impl JitterRecorder {
    pub fn new() -> Self {
        let path = trace_path();
        let (tx, rx) = mpsc::channel::<String>();
        let writer_path = path.clone();

        thread::Builder::new()
            .name("lexwright-jitter-trace".to_owned())
            .spawn(move || {
                if let Some(parent) = writer_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                let Ok(file) = File::create(&writer_path) else {
                    return;
                };

                let mut writer = BufWriter::new(file);
                let _ = writeln!(
                    writer,
                    "elapsed_us\trevision\tchanged\teditor_x\teditor_y\teditor_w\teditor_h\tgalley_x\tgalley_y\tgalley_w\tgalley_h\twrap_w\trows\tcursor_index\tcursor_row\tcursor_column\tqueued_revision\tsaved_revision\tanalysis_revision\tharper_revision\texpansion_hits"
                );

                while let Ok(line) = rx.recv() {
                    if writer.write_all(line.as_bytes()).is_err() {
                        break;
                    }
                    if writer.write_all(b"\n").is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
            })
            .expect("failed to start editor jitter trace worker");

        Self { tx, path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn record(&self, frame: JitterFrame) {
        let line = format!(
            "{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            frame.elapsed_us,
            frame.revision,
            u8::from(frame.changed),
            frame.editor_x,
            frame.editor_y,
            frame.editor_w,
            frame.editor_h,
            frame.galley_x,
            frame.galley_y,
            frame.galley_w,
            frame.galley_h,
            frame.wrap_w,
            frame.rows,
            frame.cursor_index,
            frame.cursor_row,
            frame.cursor_column,
            frame.queued_revision,
            frame.saved_revision,
            frame
                .analysis_revision
                .map_or_else(|| "-".to_owned(), |value| value.to_string()),
            frame
                .harper_revision
                .map_or_else(|| "-".to_owned(), |value| value.to_string()),
            frame.expansion_hits,
        );

        let _ = self.tx.send(line);
    }
}

fn trace_path() -> PathBuf {
    if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(state_home).join("lexwright/editor-trace.tsv");
    }

    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".local/state/lexwright/editor-trace.tsv");
    }

    PathBuf::from("lexwright-editor-trace.tsv")
}
