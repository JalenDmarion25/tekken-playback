use crate::recording::Side;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetWindowTextA, IsWindowVisible,
};

static CROSSING_TRACKER: OnceLock<Mutex<IdentityTracker>> = OnceLock::new();

#[derive(Clone, Copy)]
struct SideTracker {
    stable: Side,
    candidate: Side,
    count: u8,
}

impl SideTracker {
    fn new() -> Self {
        Self {
            stable: Side::Unknown,
            candidate: Side::Unknown,
            count: 0,
        }
    }

    fn update(&mut self, detected: Side) -> Side {
        if detected == Side::Unknown {
            return self.stable;
        }

        let needed = if self.stable == Side::Unknown || detected == self.stable {
            3
        } else {
            6
        };

        if detected == self.candidate {
            if self.count < needed {
                self.count += 1;
            }
        } else {
            self.candidate = detected;
            self.count = 1;
        }

        if self.count >= needed {
            self.stable = detected;
        }

        self.stable
    }
}

#[derive(Clone, Copy, Debug)]
struct PeakPair {
    a: f32,
    b: f32,
}

#[derive(Clone, Copy, Debug)]
struct IdentityTracker {
    stable_side: Side,
    candidate_side: Side,
    candidate_count: u8,
    me_x: Option<f32>,
    opp_x: Option<f32>,
}

impl IdentityTracker {
    fn new() -> Self {
        Self {
            stable_side: Side::Unknown,
            candidate_side: Side::Unknown,
            candidate_count: 0,
            me_x: None,
            opp_x: None,
        }
    }

    fn reset(&mut self) {
        self.stable_side = Side::Unknown;
        self.candidate_side = Side::Unknown;
        self.candidate_count = 0;
        self.me_x = None;
        self.opp_x = None;
    }

    fn initialize_from_pair(&mut self, pair: PeakPair, assumed_side: Side) -> Side {
        let (left, right) = if pair.a < pair.b {
            (pair.a, pair.b)
        } else {
            (pair.b, pair.a)
        };

        match assumed_side {
            Side::Left => {
                self.me_x = Some(left);
                self.opp_x = Some(right);
                self.stable_side = Side::Left;
            }
            Side::Right => {
                self.me_x = Some(right);
                self.opp_x = Some(left);
                self.stable_side = Side::Right;
            }
            Side::Unknown => {
                self.me_x = Some(left);
                self.opp_x = Some(right);
                self.stable_side = Side::Left;
            }
        }

        self.stable_side
    }

    fn update(&mut self, pair: Option<PeakPair>) -> Side {
        let pair = match pair {
            Some(p) => p,
            None => return self.stable_side,
        };

        if self.me_x.is_none() || self.opp_x.is_none() {
            return self.initialize_from_pair(pair, self.stable_side);
        }

        let prev_me = self.me_x.unwrap();
        let prev_opp = self.opp_x.unwrap();

        let cost_keep = (pair.a - prev_me).abs() + (pair.b - prev_opp).abs();
        let cost_swap = (pair.b - prev_me).abs() + (pair.a - prev_opp).abs();

        let (new_me, new_opp) = if cost_keep <= cost_swap {
            (pair.a, pair.b)
        } else {
            (pair.b, pair.a)
        };

        self.me_x = Some(new_me);
        self.opp_x = Some(new_opp);

        let detected = if new_me < new_opp {
            Side::Left
        } else {
            Side::Right
        };

        let needed = if self.stable_side == Side::Unknown || detected == self.stable_side {
            2
        } else {
            5
        };

        if detected == self.candidate_side {
            if self.candidate_count < needed {
                self.candidate_count += 1;
            }
        } else {
            self.candidate_side = detected;
            self.candidate_count = 1;
        }

        if self.candidate_count >= needed {
            self.stable_side = detected;
        }

        self.stable_side
    }
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

unsafe extern "system" fn find_tekken_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let mut title_buf = [0u8; 512];
    let len = GetWindowTextA(hwnd, &mut title_buf);

    if len <= 0 {
        return BOOL(1);
    }

    let title = String::from_utf8_lossy(&title_buf[..len as usize]).to_string();

    if title.contains("TEKKEN 6") {
        println!("matched window: {}", title);

        let out_ptr = lparam.0 as *mut isize;
        if !out_ptr.is_null() {
            *out_ptr = hwnd.0 as isize;
        }

        return BOOL(0); // stop enumeration
    }

    BOOL(1)
}

fn find_tekken_window() -> Option<HWND> {
    let mut found_hwnd_raw: isize = 0;

    unsafe {
        let _ = EnumWindows(
            Some(find_tekken_window_proc),
            LPARAM((&mut found_hwnd_raw as *mut isize) as isize),
        );
    }

    if found_hwnd_raw == 0 {
        None
    } else {
        Some(HWND(found_hwnd_raw as *mut c_void))
    }
}

fn capture_and_detect_once() -> Side {
    unsafe {
        let hwnd = match find_tekken_window() {
            Some(hwnd) => hwnd,
            None => {
                println!("No TEKKEN 6 window found");
                return Side::Unknown;
            }
        };

        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            println!("GetClientRect failed");
            return Side::Unknown;
        }

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;

        println!("capture size: {}x{}", width, height);

        if width <= 0 || height <= 0 {
            println!("invalid width/height");
            return Side::Unknown;
        }

        let hdc_window = GetDC(Some(hwnd));
        if hdc_window.is_invalid() {
            println!("GetDC failed");
            return Side::Unknown;
        }

        let hdc_mem = CreateCompatibleDC(Some(hdc_window));
        if hdc_mem.is_invalid() {
            println!("CreateCompatibleDC failed");
            let _ = ReleaseDC(Some(hwnd), hdc_window);
            return Side::Unknown;
        }

        let hbitmap = CreateCompatibleBitmap(hdc_window, width, height);
        if hbitmap.is_invalid() {
            println!("CreateCompatibleBitmap failed");
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(Some(hwnd), hdc_window);
            return Side::Unknown;
        }

        let old_obj = SelectObject(hdc_mem, hbitmap.into());
        if old_obj.is_invalid() {
            println!("SelectObject failed");
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(Some(hwnd), hdc_window);
            return Side::Unknown;
        }

        if BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            Some(hdc_window),
            0,
            0,
            SRCCOPY,
        )
        .is_err()
        {
            println!("BitBlt failed");
            let _ = SelectObject(hdc_mem, old_obj);
            let _ = DeleteObject(hbitmap.into());
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(Some(hwnd), hdc_window);
            return Side::Unknown;
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];

        let scanlines = GetDIBits(
            hdc_mem,
            hbitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(hdc_mem, old_obj);
        let _ = DeleteObject(hbitmap.into());
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(Some(hwnd), hdc_window);

        println!("scanlines: {}", scanlines);

        if scanlines == 0 {
            println!("GetDIBits returned 0");
            return Side::Unknown;
        }

        let side = detect_side_from_frame_rgba(&buffer, width as u32, height as u32);
        println!("raw detected side: {:?}", side);
        side
    }
}

pub fn detect_side_once() -> Side {
    capture_and_detect_once()
}

pub fn detect_side_from_frame_rgba(rgba: &[u8], width: u32, height: u32) -> Side {
    if width == 0 || height == 0 {
        return Side::Unknown;
    }

    let expected_len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);

    if rgba.len() < expected_len {
        return Side::Unknown;
    }

    let peaks = detect_fighter_pair(rgba, width, height);

    if let Some(p) = peaks {
        println!("peaks: a={:.1} b={:.1}", p.a, p.b);
    } else {
        println!("peaks: none");
    }

    let tracker = CROSSING_TRACKER.get_or_init(|| Mutex::new(IdentityTracker::new()));
    let mut tracker = tracker.lock().unwrap();

    let side = tracker.update(peaks);

    println!(
        "tracked: me_x={:?} opp_x={:?} stable_side={:?}",
        tracker.me_x, tracker.opp_x, side
    );

    side
}

fn roi_fighter_score(rgba: &[u8], width: u32, height: u32, rect: Rect) -> u32 {
    let x0 = rect.x.min(width);
    let y0 = rect.y.min(height);
    let x1 = rect.x.saturating_add(rect.w).min(width);
    let y1 = rect.y.saturating_add(rect.h).min(height);

    if x0 >= x1 || y0 >= y1 {
        return 0;
    }

    let mut strong_pixels: u64 = 0;
    let mut total_energy: u64 = 0;
    let mut count: u64 = 0;

    // Sample every 2 pixels for speed and a little noise resistance.
    let step: usize = 2;

    let get_rgb = |x: u32, y: u32| -> Option<(i32, i32, i32)> {
        if x >= width || y >= height {
            return None;
        }
        let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
        if idx + 3 >= rgba.len() {
            return None;
        }
        Some((rgba[idx] as i32, rgba[idx + 1] as i32, rgba[idx + 2] as i32))
    };

    let mut y = y0 as usize;
    while y + step < y1 as usize {
        let mut x = x0 as usize;
        while x + step < x1 as usize {
            let (r, g, b) = match get_rgb(x as u32, y as u32) {
                Some(v) => v,
                None => {
                    x += step;
                    continue;
                }
            };

            let brightness = (r + g + b) / 3;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let saturation = max_c - min_c;

            let (r2, g2, b2) = match get_rgb((x + step) as u32, y as u32) {
                Some(v) => v,
                None => (r, g, b),
            };
            let (r3, g3, b3) = match get_rgb(x as u32, (y + step) as u32) {
                Some(v) => v,
                None => (r, g, b),
            };

            let brightness_x = (r2 + g2 + b2) / 3;
            let brightness_y = (r3 + g3 + b3) / 3;

            let edge = (brightness - brightness_x).abs() + (brightness - brightness_y).abs();

            // Weight colorful / contrasted / non-dark foreground-ish pixels.
            let mut energy = 0u32;

            if brightness > 28 {
                energy += (brightness as u32) / 3;
            }

            if saturation > 18 {
                energy += saturation as u32;
            }

            if edge > 20 {
                energy += (edge as u32) * 2;
                strong_pixels += 1;
            }

            total_energy += energy as u64;
            count += 1;

            x += step;
        }
        y += step;
    }

    if count == 0 {
        return 0;
    }

    let avg_energy = total_energy / count;
    let strong_bonus = strong_pixels * 2;

    (avg_energy + strong_bonus) as u32
}

fn detect_fighter_pair(rgba: &[u8], width: u32, height: u32) -> Option<PeakPair> {
    if width < 64 || height < 64 {
        return None;
    }

    let y0 = height * 30 / 100;
    let y1 = height * 68 / 100;
    let x0 = width * 15 / 100;
    let x1 = width * 85 / 100;

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let span = (x1 - x0) as usize;
    let mut hist = vec![0u32; span];

    let step_x = 2usize;
    let step_y = 3usize;

    for y in (y0 as usize..y1 as usize).step_by(step_y) {
        for x in (x0 as usize..x1 as usize).step_by(step_x) {
            let idx = (y * width as usize + x) * 4;
            if idx + 3 >= rgba.len() {
                continue;
            }

            let r = rgba[idx] as i32;
            let g = rgba[idx + 1] as i32;
            let b = rgba[idx + 2] as i32;

            let brightness = (r + g + b) / 3;
            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            let saturation = max_c - min_c;

            let edge = if x + step_x < x1 as usize {
                let idx2 = (y * width as usize + (x + step_x)) * 4;
                if idx2 + 3 < rgba.len() {
                    let r2 = rgba[idx2] as i32;
                    let g2 = rgba[idx2 + 1] as i32;
                    let b2 = rgba[idx2 + 2] as i32;
                    let b2v = (r2 + g2 + b2) / 3;
                    (brightness - b2v).abs()
                } else {
                    0
                }
            } else {
                0
            };

            let mut energy = 0u32;

            if brightness > 35 {
                energy += (brightness as u32) / 4;
            }
            if saturation > 20 {
                energy += saturation as u32;
            }
            if edge > 18 {
                energy += (edge as u32) * 3;
            }

            hist[x - x0 as usize] += energy;
        }
    }

    smooth_histogram(&mut hist, 12);

    let (p1, p2) = find_top_two_peaks(&hist, (width as usize * 10 / 100).max(60))?;
    let a = (x0 as usize + p1) as f32;
    let b = (x0 as usize + p2) as f32;

    if (a - b).abs() < width as f32 * 0.10 {
        return None;
    }

    Some(PeakPair { a, b })
}

fn find_top_two_peaks(hist: &[u32], min_separation: usize) -> Option<(usize, usize)> {
    if hist.len() < 2 {
        return None;
    }

    let mut best1_idx = None;
    let mut best1_val = 0u32;

    for (i, &v) in hist.iter().enumerate() {
        if v > best1_val {
            best1_val = v;
            best1_idx = Some(i);
        }
    }

    let best1 = best1_idx?;

    let mut best2_idx = None;
    let mut best2_val = 0u32;

    for (i, &v) in hist.iter().enumerate() {
        if i.abs_diff(best1) < min_separation {
            continue;
        }
        if v > best2_val {
            best2_val = v;
            best2_idx = Some(i);
        }
    }

    let best2 = best2_idx?;

    let (a, b) = if best1 < best2 {
        (best1, best2)
    } else {
        (best2, best1)
    };

    Some((a, b))
}

fn smooth_histogram(hist: &mut Vec<u32>, radius: usize) {
    if hist.is_empty() || radius == 0 {
        return;
    }

    let src = hist.clone();

    for i in 0..hist.len() {
        let start = i.saturating_sub(radius);
        let end = (i + radius + 1).min(src.len());

        let mut sum = 0u64;
        let mut count = 0u64;

        for v in &src[start..end] {
            sum += *v as u64;
            count += 1;
        }

        hist[i] = if count > 0 { (sum / count) as u32 } else { 0 };
    }
}
