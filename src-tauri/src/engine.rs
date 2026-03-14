use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{Frame, Recording};

#[derive(Default)]
pub struct AppState {
    pub recordings: Vec<Option<Recording>>,
    pub selected_slot: usize,
    pub is_recording: bool,
}

#[derive(Clone)]
pub struct Engine {
    state: Arc<Mutex<AppState>>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState {
                recordings: vec![None, None, None, None, None],
                selected_slot: 0,
                is_recording: false,
            })),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.state.lock().unwrap().is_recording
    }

    pub fn set_selected_slot(&self, slot: usize) -> Result<(), String> {
        if slot >= 5 {
            return Err("Invalid slot".into());
        }
        let mut st = self.state.lock().unwrap();
        st.selected_slot = slot;
        Ok(())
    }

    pub fn selected_slot(&self) -> usize {
        self.state.lock().unwrap().selected_slot
    }

    pub fn has_recording_in_selected_slot(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.recordings
            .get(st.selected_slot)
            .and_then(|r| r.as_ref())
            .map(|r| !r.frames.is_empty())
            .unwrap_or(false)
    }

    pub fn slot_has_recording(&self, slot: usize) -> bool {
        let st = self.state.lock().unwrap();
        st.recordings
            .get(slot)
            .and_then(|r| r.as_ref())
            .map(|r| !r.frames.is_empty())
            .unwrap_or(false)
    }

    /// Start capturing frames from XInput controller `controller_index` at `fps`.
    pub fn start_recording(&self, fps: u32) {
        let mut st = self.state.lock().unwrap();
        let slot = st.selected_slot;
        st.recordings[slot] = Some(Recording {
            fps,
            frames: Vec::new(),
        });
        st.is_recording = true;
    }

    pub fn push_frame_if_recording(&self, frame: Frame) {
        let mut st = self.state.lock().unwrap();
        if !st.is_recording {
            return;
        }

        let slot = st.selected_slot;
        if let Some(rec) = st.recordings[slot].as_mut() {
            rec.frames.push(frame);
        }
    }

    /// Stop recording (does not delete it).
    pub fn stop_recording(&self) {
        let mut st = self.state.lock().unwrap();
        st.is_recording = false;
    }

    /// Get a clone of the current recording (if any).
    pub fn get_selected_recording(&self) -> Option<Recording> {
        let st = self.state.lock().unwrap();
        st.recordings.get(st.selected_slot).cloned().flatten()
    }

    /// Save current recording to a JSON file.
    pub fn save_recording<P: AsRef<Path>>(&self, path: P) -> std::io::Result<PathBuf> {
        let rec = self.get_selected_recording().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "No recording to save")
        })?;

        let json = serde_json::to_string_pretty(&rec)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, json)?;
        Ok(path.to_path_buf())
    }

    /// Load a recording JSON file and set it as the current recording.
    pub fn load_recording<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let bytes = fs::read(path)?;
        let rec: Recording = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let mut st = self.state.lock().unwrap();
        let slot = st.selected_slot;
        st.recordings[slot] = Some(rec);
        st.is_recording = false;
        Ok(())
    }

    /// Playback the current recording by iterating frames at the recorded fps.
    /// For now, this just calls `on_frame(frame)` each tick (we’ll later send to ViGEm).
    pub fn playback<F>(&self, mut on_frame: F) -> Result<(), String>
    where
        F: FnMut(Frame),
    {
        let rec = self.get_selected_recording().ok_or("No recording loaded")?;
        let fps = rec.fps.max(1);
        let frame_time = Duration::from_secs_f64(1.0 / fps as f64);

        let mut next_tick = std::time::Instant::now();

        for frame in rec.frames {
            next_tick += frame_time;
            on_frame(frame);

            let now = std::time::Instant::now();
            if next_tick > now {
                std::thread::sleep(next_tick - now);
            } else {
                next_tick = now;
            }
        }

        Ok(())
    }
}
