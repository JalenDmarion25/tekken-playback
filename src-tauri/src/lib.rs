use once_cell::sync::Lazy;
use rand::seq::SliceRandom;
use std::sync::Mutex;

use crate::engine::Engine;
use crate::vigem::X360Pad;
pub mod engine;
pub mod input;
pub mod recording;
pub mod side_detection;
pub mod vigem;

pub use recording::{Frame, Recording, Side};

#[derive(Clone, Copy)]
enum RouteMode {
    Normal,          // input -> P1
    SwapWhileRecord, // input -> P2 (during recording)
}

struct AppState {
    engine: Engine,
    pad_p1: X360Pad,
    pad_p2: X360Pad,
    route: RouteMode,
    p1_frozen: bool,
    is_playing: bool,
    stop_playback: bool,
    repeat_playback: bool,
    detected_side: Side,
}

static APP: Lazy<Mutex<AppState>> = Lazy::new(|| {
    let engine = Engine::new();

    let pad_p1 = X360Pad::new().expect("ViGEm init failed (install ViGEmBus)");
    let pad_p2 = X360Pad::new().expect("ViGEm init failed (install ViGEmBus)");

    Mutex::new(AppState {
        engine,
        pad_p1,
        pad_p2,
        route: RouteMode::Normal,
        p1_frozen: false,
        is_playing: false,
        stop_playback: false,
        repeat_playback: false,
        detected_side: Side::Unknown,
    })
});

#[derive(serde::Serialize)]
struct Status {
    status: String,
    is_recording: bool,
    has_recording: bool,
    is_playing: bool,
    repeat_playback: bool,
    selected_slot: usize,
    slots: Vec<bool>,
    current_side: String,
}

const DPAD_LEFT: u16 = 0x0004;
const DPAD_RIGHT: u16 = 0x0008;

fn invert_horizontal_buttons(buttons: u16) -> u16 {
    let left = buttons & DPAD_LEFT != 0;
    let right = buttons & DPAD_RIGHT != 0;

    let mut out = buttons & !(DPAD_LEFT | DPAD_RIGHT);

    if left {
        out |= DPAD_RIGHT;
    }
    if right {
        out |= DPAD_LEFT;
    }

    out
}

fn invert_frame_horizontal(frame: Frame) -> Frame {
    Frame {
        buttons: invert_horizontal_buttons(frame.buttons),
        lx: frame.lx.saturating_neg(),
        ..frame
    }
}

#[tauri::command]
fn get_status() -> Status {
    let app = APP.lock().unwrap();

    let selected_slot = app.engine.selected_slot();
    let slots = (0..5)
        .map(|i| app.engine.slot_has_recording(i))
        .collect::<Vec<_>>();

    Status {
        status: if app.engine.is_recording() {
            "Recording...".into()
        } else if app.is_playing {
            "Playing...".into()
        } else {
            format!("Idle - Slot {}", selected_slot + 1)
        },
        is_recording: app.engine.is_recording(),
        has_recording: app.engine.has_recording_in_selected_slot(),
        is_playing: app.is_playing,
        repeat_playback: app.repeat_playback,
        selected_slot,
        slots,
        current_side: match app.detected_side {
            Side::Left => "left".into(),
            Side::Right => "right".into(),
            Side::Unknown => "unknown".into(),
        },
    }
}

#[tauri::command]
fn start_recording(_controller_index: u32, fps: u32, max_seconds: u32) -> Result<(), String> {
    let (engine, side) = {
        let app = APP.lock().unwrap();
        (app.engine.clone(), app.detected_side)
    };

    if side == Side::Unknown {
        return Err("Side detection is not ready yet.".into());
    }

    {
        let mut app = APP.lock().unwrap();
        app.route = RouteMode::SwapWhileRecord;
        app.p1_frozen = true;

        // Immediately neutralize P1
        let _ = app.pad_p1.reset();
    }

    engine.start_recording(fps, side);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(max_seconds as u64));
        let mut app = APP.lock().unwrap();
        app.engine.stop_recording();
        app.route = RouteMode::Normal;
        app.p1_frozen = false;
    });

    Ok(())
}

#[tauri::command]
fn stop_recording() -> Result<(), String> {
    let mut app = APP.lock().unwrap();
    app.engine.stop_recording();
    app.route = RouteMode::Normal;
    app.p1_frozen = false;
    Ok(())
}

#[tauri::command]
fn set_repeat_playback(enabled: bool) -> Result<(), String> {
    let mut app = APP.lock().unwrap();
    app.repeat_playback = enabled;
    Ok(())
}

#[tauri::command]
fn stop_playback() -> Result<(), String> {
    let mut app = APP.lock().unwrap();
    app.stop_playback = true;
    Ok(())
}

#[tauri::command]
fn random_playback() -> Result<(), String> {
    {
        let mut app = APP.lock().unwrap();
        if app.is_playing {
            return Ok(());
        }
        app.is_playing = true;
        app.stop_playback = false;
    }

    std::thread::spawn(|| loop {
        let (_slot, rec, repeat) = {
            let app = APP.lock().unwrap();
            let slots = app.engine.filled_slots();

            if slots.is_empty() {
                drop(app);
                let mut app = APP.lock().unwrap();
                app.is_playing = false;
                return;
            }

            let mut rng = rand::thread_rng();
            let slot = *slots.choose(&mut rng).unwrap();
            let rec = match app.engine.get_recording_for_slot(slot) {
                Some(r) => r,
                None => {
                    drop(app);
                    let mut app = APP.lock().unwrap();
                    app.is_playing = false;
                    return;
                }
            };

            (slot, rec, app.repeat_playback)
        };

        {
            let mut app = APP.lock().unwrap();
            let _ = app.pad_p2.reset();
        }

        let fps = rec.fps.max(1);
        let frame_time = std::time::Duration::from_secs_f64(1.0 / fps as f64);
        let mut next_tick = std::time::Instant::now();

        for frame in rec.frames {
            {
                let app = APP.lock().unwrap();
                if app.stop_playback {
                    drop(app);
                    let mut app = APP.lock().unwrap();
                    let _ = app.pad_p2.reset();
                    app.is_playing = false;
                    app.stop_playback = false;
                    return;
                }
            }

            next_tick += frame_time;

            {
                let mut app = APP.lock().unwrap();
                let frame_to_send = if rec.side != Side::Unknown
                    && app.detected_side != Side::Unknown
                    && rec.side != app.detected_side
                {
                    invert_frame_horizontal(frame)
                } else {
                    frame
                };

                let _ = app.pad_p2.set_frame(frame_to_send);
            }

            let now = std::time::Instant::now();
            if next_tick > now {
                std::thread::sleep(next_tick - now);
            } else {
                next_tick = now;
            }
        }

        {
            let mut app = APP.lock().unwrap();
            let _ = app.pad_p2.reset();
        }

        if !repeat {
            let mut app = APP.lock().unwrap();
            app.is_playing = false;
            app.stop_playback = false;
            return;
        }
    });

    Ok(())
}

#[tauri::command]
fn playback() -> Result<(), String> {
    {
        let mut app = APP.lock().unwrap();

        if app.is_playing {
            return Ok(());
        }

        app.is_playing = true;
        app.stop_playback = false;
    }

    std::thread::spawn(|| {
        let rec = {
            let app = APP.lock().unwrap();
            match app.engine.get_selected_recording() {
                Some(r) => r,
                None => {
                    drop(app);
                    let mut app = APP.lock().unwrap();
                    app.is_playing = false;
                    return;
                }
            }
        };

        let fps = rec.fps.max(1);
        let frame_time = std::time::Duration::from_secs_f64(1.0 / fps as f64);

        loop {
            {
                let mut app = APP.lock().unwrap();
                let _ = app.pad_p2.reset();
            }

            let mut next_tick = std::time::Instant::now();

            for frame in &rec.frames {
                {
                    let app = APP.lock().unwrap();
                    if app.stop_playback {
                        drop(app);
                        let mut app = APP.lock().unwrap();
                        let _ = app.pad_p2.reset();
                        app.is_playing = false;
                        app.stop_playback = false;
                        return;
                    }
                }

                next_tick += frame_time;

                {
                    let mut app = APP.lock().unwrap();

                    let frame_to_send = if rec.side != Side::Unknown
                        && app.detected_side != Side::Unknown
                        && rec.side != app.detected_side
                    {
                        invert_frame_horizontal(*frame)
                    } else {
                        *frame
                    };

                    let _ = app.pad_p2.set_frame(frame_to_send);
                }

                let now = std::time::Instant::now();
                if next_tick > now {
                    std::thread::sleep(next_tick - now);
                } else {
                    next_tick = now;
                }
            }

            let repeat = {
                let app = APP.lock().unwrap();
                app.repeat_playback
            };

            if !repeat {
                let mut app = APP.lock().unwrap();
                let _ = app.pad_p2.reset();
                app.is_playing = false;
                app.stop_playback = false;
                return;
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn set_selected_slot(slot: usize) -> Result<(), String> {
    let app = APP.lock().unwrap();
    app.engine.set_selected_slot(slot)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::thread::spawn(|| {
        let controller_index = 0;
        let fps = 60;

        let _ = crate::input::poll_xinput(controller_index, fps, |frame| {
            let mut app = APP.lock().unwrap();

            // route live input
            match app.route {
                RouteMode::Normal => {
                    let _ = app.pad_p1.set_frame(frame);
                }
                RouteMode::SwapWhileRecord => {
                    let _ = app.pad_p2.set_frame(frame);
                }
            }

            // also record if recording is active
            app.engine.push_frame_if_recording(frame);
        });
    });

    std::thread::spawn(|| loop {
        let side = crate::side_detection::detect_side_once();

        {
            let mut app = APP.lock().unwrap();
            app.detected_side = side;
        }

        println!("detected side: {:?}", side);

        std::thread::sleep(std::time::Duration::from_millis(33));
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_status,
            start_recording,
            stop_recording,
            playback,
            stop_playback,
            set_repeat_playback,
            set_selected_slot,
            random_playback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
