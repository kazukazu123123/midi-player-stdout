use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use midi_toolkit::{
    events::MIDIEvent,
    io::MIDIFile,
    pipe,
    sequence::{
        event::{
            cancel_tempo_events, get_channels_array_statistics, merge_events_array,
            scale_event_time,
        },
        to_vec, unwrap_items, TimeCaster,
    },
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path of the midi file
    #[arg(short, long)]
    midi_file: String,
    /// Log interval in milliseconds
    #[arg(short, long, default_value_t = 500)]
    log_interval: u64,
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs_f64();
    let hours = (total_seconds / 3600.0) as u64;
    let minutes = ((total_seconds % 3600.0) / 60.0) as u64;
    let seconds = (total_seconds % 60.0) as u64;

    let ms = (total_seconds.fract() * 100.0) as u64;
    format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, ms)
}

fn main() {
    let args = Args::parse();

    eprintln!("evt_parsing");

    let midi = MIDIFile::open(args.midi_file, None).unwrap();

    eprintln!("evt_parsed");

    let ppq = midi.ppq();
    let merge_midi = || {
        pipe!(
            midi.iter_all_tracks()
            |>to_vec()
            |>merge_events_array()
            |>TimeCaster::<f64>::cast_event_delta()
            |>cancel_tempo_events(250000)
            |>scale_event_time(1.0 / ppq as f64)
            |>unwrap_items()
        )
    };

    let statistics = pipe!(
        midi.iter_all_tracks()
        |>to_vec()
        |>get_channels_array_statistics()
    )
    .expect("Failed to calculate statistics we're doomed");
    let midi_duration = statistics.calculate_total_duration(ppq);
    let note_count = statistics.note_count();

    eprintln!("evt_note_count,{}", note_count);
    eprintln!("evt_duration_ns,{}", midi_duration.as_nanos());
    eprintln!("evt_duration_formatted,{}", format_duration(midi_duration));

    let merged = merge_midi();

    let now = Instant::now();
    let start_time = now.clone();
    let is_playing = Arc::new(Mutex::new(true));
    let is_playing_for_thread = is_playing.clone();

    let progress_thread = thread::spawn(move || {
        while *is_playing_for_thread.lock().unwrap() {
            let real_elapsed = start_time.elapsed().as_secs_f64();

            let elapsed = real_elapsed.min(midi_duration.as_secs_f64());
            let percent = (elapsed / midi_duration.as_secs_f64()) * 100.0;

            eprintln!(
                "evt_progress,position_sec={:.3},total_sec={:.3},position_fmt={},total_fmt={},percent={:.1}",
                elapsed,
                midi_duration.as_secs_f64(),
                format_duration(Duration::from_secs_f64(elapsed)),
                format_duration(midi_duration),
                percent
            );

            thread::sleep(Duration::from_millis(args.log_interval));
        }
    });

    eprintln!("evt_playing");

    let mut time = 0.0;

    for e in merged {
        if e.delta != 0.0 {
            time += e.delta;
            let diff = time - now.elapsed().as_secs_f64();

            if diff > 0.0 {
                thread::sleep(Duration::from_secs_f64(diff));
            }
        }

        if let Some(serialized) = e.as_u32() {
            let mut chunks: Vec<String> = Vec::new();

            for i in (0..24).step_by(8) {
                let chunk = ((serialized >> i) & 0xFF) as u8;
                chunks.push(format!("{:02X}", chunk));
            }

            let joined = chunks.join(",");
            println!("{}", joined);
        }
    }

    *is_playing.lock().unwrap() = false;

    if let Err(e) = progress_thread.join() {
        eprintln!("Error joining progress thread: {:?}", e);
    }

    eprintln!("evt_playing_finished");
}
