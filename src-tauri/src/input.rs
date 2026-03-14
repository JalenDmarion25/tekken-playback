use std::time::{Duration, Instant};

use windows::Win32::UI::Input::XboxController::{
    XINPUT_GAMEPAD, XINPUT_STATE, XInputGetState,
};

use crate::recording::Frame;

#[derive(Debug)]
pub enum InputError {
    NotConnected(u32),
} 

fn apply_deadzone_i16(v: i16, deadzone: i16) -> i16 {
    if v.abs() <= deadzone { 0 } else { v }
}

/// Poll an XInput controller (0..=3) at `fps` and call `on_frame` each tick.
/// Returns an error if the controller is not connected at start.
pub fn poll_xinput<F>(controller_index: u32, fps: u32, mut on_frame: F) -> Result<(), InputError>
where
    F: FnMut(Frame),
{
    if controller_index > 3 {
        return Err(InputError::NotConnected(controller_index));
    }

    // Check connected once up front
    let mut state = XINPUT_STATE::default();
    let res = unsafe { XInputGetState(controller_index, &mut state) };
    if res != 0 {
        return Err(InputError::NotConnected(controller_index));
    }

    let frame_time = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_tick = Instant::now();

    loop {
        next_tick += frame_time;

        let mut state = XINPUT_STATE::default();
        let res = unsafe { XInputGetState(controller_index, &mut state) };

        // If unplugged / disconnected mid-run, we just skip frames
        if res == 0 {
            let frame = xinput_state_to_frame(&state.Gamepad);
            on_frame(frame);
        }

        // Drift-corrected sleep
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        }
    }
}

/// Convert XINPUT_GAMEPAD -> your Frame type.
fn xinput_state_to_frame(gp: &XINPUT_GAMEPAD) -> Frame {
    // Typical XInput deadzones (rough defaults)
    const LEFT_DEADZONE: i16 = 7849;
    const RIGHT_DEADZONE: i16 = 8689;

    Frame {
        buttons: gp.wButtons.0,
        lt: gp.bLeftTrigger,
        rt: gp.bRightTrigger,
        lx: apply_deadzone_i16(gp.sThumbLX, LEFT_DEADZONE),
        ly: apply_deadzone_i16(gp.sThumbLY, LEFT_DEADZONE),
        rx: apply_deadzone_i16(gp.sThumbRX, RIGHT_DEADZONE),
        ry: apply_deadzone_i16(gp.sThumbRY, RIGHT_DEADZONE),
    }
}

/// Utility: pretty-print a frame for debugging.
pub fn describe_frame(f: &Frame) -> String {
    let mut parts: Vec<&str> = Vec::new();

    // XInput button bits
    const DPAD_UP: u16 = 0x0001;
    const DPAD_DOWN: u16 = 0x0002;
    const DPAD_LEFT: u16 = 0x0004;
    const DPAD_RIGHT: u16 = 0x0008;
    const START: u16 = 0x0010;
    const BACK: u16 = 0x0020;
    const LS: u16 = 0x0040;
    const RS: u16 = 0x0080;
    const LB: u16 = 0x0100;
    const RB: u16 = 0x0200;
    const A: u16 = 0x1000;
    const B: u16 = 0x2000;
    const X: u16 = 0x4000;
    const Y: u16 = 0x8000;

    let b = f.buttons;
    if b & DPAD_UP != 0 { parts.push("Up"); }
    if b & DPAD_DOWN != 0 { parts.push("Down"); }
    if b & DPAD_LEFT != 0 { parts.push("Left"); }
    if b & DPAD_RIGHT != 0 { parts.push("Right"); }
    if b & START != 0 { parts.push("Start"); }
    if b & BACK != 0 { parts.push("Back"); }
    if b & LS != 0 { parts.push("L3"); }
    if b & RS != 0 { parts.push("R3"); }
    if b & LB != 0 { parts.push("LB"); }
    if b & RB != 0 { parts.push("RB"); }
    if b & A != 0 { parts.push("A"); }
    if b & B != 0 { parts.push("B"); }
    if b & X != 0 { parts.push("X"); }
    if b & Y != 0 { parts.push("Y"); }

    format!(
        "buttons=[{}] lt={} rt={} lx={} ly={} rx={} ry={}",
        parts.join(" "),
        f.lt, f.rt, f.lx, f.ly, f.rx, f.ry
    )
}