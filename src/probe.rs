use std::{hint::black_box, time::Instant};

use eframe::egui::{TextBuffer, text::CharIndex};

use crate::editor_buffer::EditorBuffer;

const SIZES: &[usize] = &[4 * 1024, 64 * 1024, 1024 * 1024, 8 * 1024 * 1024];

pub fn run() {
    println!("Lexwright latency probe");
    println!("CPU-side mutation timings; lower is better.");
    println!("Run with --release for meaningful numbers.");

    if cfg!(debug_assertions) {
        println!("WARNING: this is a debug build; results are not representative.");
    }

    println!();
    println!(
        "{:>10}  {:>14}  {:>14}  {:>14}",
        "ledger", "append edit", "middle edit", "snapshot clone"
    );

    for &size in SIZES {
        let mut buffer = EditorBuffer::new("a".repeat(size));
        buffer.set_expansions_enabled(false);

        // Warm allocation/call paths before timing.
        let end = size;
        buffer.insert_text("x", CharIndex(end));
        buffer.delete_char_range(CharIndex(end)..CharIndex(end + 1));

        let append_ns = bench_edit_cycle(&mut buffer, end, 2_000);
        let middle_iterations = if size <= 64 * 1024 {
            500
        } else if size <= 1024 * 1024 {
            100
        } else {
            20
        };
        let middle_ns = bench_edit_cycle(&mut buffer, size / 2, middle_iterations);

        let clone_iterations = if size <= 64 * 1024 {
            500
        } else if size <= 1024 * 1024 {
            100
        } else {
            20
        };
        let clone_ns = bench_snapshot_clone(&buffer, clone_iterations);

        println!(
            "{:>10}  {:>14}  {:>14}  {:>14}",
            format_bytes(size),
            format_ns(append_ns),
            format_ns(middle_ns),
            format_ns(clone_ns),
        );
    }

    println!();
    println!("Interpretation:");
    println!("- append edit approximates the normal ledger-at-end writing path");
    println!("- middle edit exposes contiguous-String byte shifting");
    println!("- snapshot clone is the remaining UI-thread autosave-copy cost");
    println!("- this does not measure keyboard, compositor, scanout, or display latency");
}

fn bench_edit_cycle(buffer: &mut EditorBuffer, char_index: usize, iterations: usize) -> u64 {
    let mut total_ns = 0_u128;

    for _ in 0..iterations {
        let started = Instant::now();
        let advance = buffer.insert_text("x", CharIndex(char_index));
        total_ns = total_ns.saturating_add(started.elapsed().as_nanos());
        black_box(advance);

        buffer.delete_char_range(CharIndex(char_index)..CharIndex(char_index.saturating_add(1)));
    }

    average_ns(total_ns, iterations)
}

fn bench_snapshot_clone(buffer: &EditorBuffer, iterations: usize) -> u64 {
    let started = Instant::now();

    for _ in 0..iterations {
        black_box(buffer.text().to_owned());
    }

    average_ns(started.elapsed().as_nanos(), iterations)
}

fn average_ns(total_ns: u128, iterations: usize) -> u64 {
    if iterations == 0 {
        return 0;
    }

    (total_ns / iterations as u128).min(u64::MAX as u128) as u64
}

fn format_ns(nanos: u64) -> String {
    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.1} us", nanos as f64 / 1_000.0)
    } else {
        format!("{:.2} ms", nanos as f64 / 1_000_000.0)
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} MiB", bytes / (1024 * 1024))
    }
}
