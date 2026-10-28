//! Audio cues: the game's own sounds that mark a moment the picture can't date exactly.
//!
//! While a reader asks for it, the app records Aniimo's audio only (Windows' per-app loopback, Windows 10 2004+;
//! nothing else playing on the PC is heard) and listens for the boss kill sound. Nothing is recorded or kept: each
//! slice of audio is turned into a spectrum, compared, and dropped.
//!
//! Boss kill: the game's `SFX_Combat_Boss_FirstKill` plays with the killing blow, ~3.6 s before the Catch Countdown.
//! `boss_kill.fp` is its first 0.8 s as 80 spectrum frames (2048-point FFT every 10 ms at 48 kHz, 1.5-14 kHz,
//! log magnitude minus each frame's mean, i8 per bin). A match is the mean per-frame cosine of the last 80 frames
//! against it: 0.63 at a recorded kill, never above 0.44 through the rest of that fight.
//!
//! Layout of `Local\aLogs.Audio.v1` (little-endian):
//!   0  u32 magic "ALAU"      4  u32 version (1)
//!   8  u64 wanted until (unix ms): readers keep it a couple of seconds ahead while they want cues
//!   16 u64 last boss kill sound (unix ms, when it started)   24 f32 its match score   28 u32 count (wraps)
//!   32 u32 state: 0 not listening, 1 listening, 2 can't listen (no game, or Windows refused)
//!   36 u32 game process id   40 u64 last audio heard (unix ms)
//! A reader takes a cue when the count changes; the count is written last.

use std::collections::VecDeque;
use std::ptr::null;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Memory::{CreateFileMappingW, MapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

pub const NAME: &str = "Local\\aLogs.Audio.v1";
const MAGIC: u32 = u32::from_le_bytes(*b"ALAU");
const SIZE: usize = 4096;

const RATE: usize = 48_000;
const N: usize = 2048;
const HOP: usize = 480;
const K0: usize = 65; // 1.5 kHz
const K1: usize = 598; // 14 kHz
const BINS: usize = K1 - K0;
const FRAMES: usize = 80;
const THRESHOLD: f32 = 0.55;
const FINGERPRINT: &[u8; FRAMES * BINS] = include_bytes!("boss_kill.fp");

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Shared {
    base: *mut u8,
}

impl Shared {
    fn create() -> Option<Shared> {
        let name: Vec<u16> = NAME.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let handle = CreateFileMappingW(INVALID_HANDLE_VALUE, null(), PAGE_READWRITE, 0, SIZE as u32, name.as_ptr());
            if handle.is_null() {
                return None;
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, SIZE);
            if view.Value.is_null() {
                CloseHandle(handle);
                return None;
            }
            // The mapping lives as long as the app: the handle is never closed on purpose.
            let shared = Shared { base: view.Value as *mut u8 };
            shared.put_u32(0, MAGIC);
            shared.put_u32(4, 1);
            Some(shared)
        }
    }
    fn put_u32(&self, at: usize, value: u32) {
        unsafe { (self.base.add(at) as *mut u32).write_volatile(value) }
    }
    fn put_u64(&self, at: usize, value: u64) {
        unsafe { (self.base.add(at) as *mut u64).write_unaligned(value) }
    }
    fn get_u32(&self, at: usize) -> u32 {
        unsafe { (self.base.add(at) as *const u32).read_volatile() }
    }
    fn get_u64(&self, at: usize) -> u64 {
        unsafe { (self.base.add(at) as *const u64).read_volatile() }
    }
    fn wanted(&self, now: u64) -> bool {
        self.get_u64(8) > now
    }
    fn cue(&self, at_ms: u64, score: f32) {
        self.put_u64(16, at_ms);
        self.put_u32(24, score.to_bits());
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
        self.put_u32(28, self.get_u32(28).wrapping_add(1));
    }
}

// The view is only written from the audio thread (readers write only "wanted until").
unsafe impl Send for Shared {}

/// Spectrum frames of the game's audio, compared with the fingerprint as they come.
struct Listener {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    print: Vec<[f32; BINS]>,
    samples: VecDeque<f32>, // mono, not yet framed (the oldest sample is `start_ms` old)
    frames: VecDeque<[f32; BINS]>,
    frame_ms: VecDeque<u64>,
    best: Option<(f32, u64, u32)>, // a match being followed: (score, sound start, frames since its peak)
    last_cue: u64,
}

impl Listener {
    fn new() -> Listener {
        let window = (0..N).map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / (N - 1) as f32).cos()).collect();
        let print = FINGERPRINT
            .chunks(BINS)
            .map(|row| {
                let mut v = [0f32; BINS];
                for (o, &b) in v.iter_mut().zip(row) {
                    *o = b as i8 as f32;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                v.iter_mut().for_each(|x| *x /= norm);
                v
            })
            .collect();
        Listener {
            fft: FftPlanner::new().plan_fft_forward(N),
            window,
            print,
            samples: VecDeque::new(),
            frames: VecDeque::new(),
            frame_ms: VecDeque::new(),
            best: None,
            last_cue: 0,
        }
    }

    fn reset(&mut self) {
        self.samples.clear();
        self.frames.clear();
        self.frame_ms.clear();
        self.best = None;
    }

    /// A packet of mono samples that ended at `end_ms`. Returns a boss kill sound once its match has peaked.
    fn feed(&mut self, mono: &[f32], end_ms: u64) -> Option<(u64, f32)> {
        self.samples.extend(mono);
        let mut found = None;
        let mut buf = vec![Complex { re: 0.0, im: 0.0 }; N];
        while self.samples.len() >= N {
            // when this frame started: the packet's end minus everything after the frame's first sample
            let start_ms = end_ms.saturating_sub((self.samples.len() * 1000 / RATE) as u64);
            for (i, (c, s)) in buf.iter_mut().zip(self.samples.iter()).enumerate() {
                *c = Complex { re: s * self.window[i], im: 0.0 };
            }
            self.fft.process(&mut buf);
            let mut row = [0f32; BINS];
            for (o, c) in row.iter_mut().zip(&buf[K0..K1]) {
                *o = (c.norm() + 1e-6).log10();
            }
            let mean = row.iter().sum::<f32>() / BINS as f32;
            row.iter_mut().for_each(|x| *x -= mean);
            let norm = row.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
            row.iter_mut().for_each(|x| *x /= norm);
            self.frames.push_back(row);
            self.frame_ms.push_back(start_ms);
            if self.frames.len() > FRAMES {
                self.frames.pop_front();
                self.frame_ms.pop_front();
            }
            self.samples.drain(..HOP);
            if self.frames.len() == FRAMES {
                if let Some(cue) = self.check() {
                    found = Some(cue);
                }
            }
        }
        found
    }

    fn check(&mut self) -> Option<(u64, f32)> {
        let score = self
            .frames
            .iter()
            .zip(&self.print)
            .map(|(a, b)| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>())
            .sum::<f32>()
            / FRAMES as f32;
        let start = self.frame_ms[0];
        match self.best {
            Some((best, _, _)) if score > best => self.best = Some((score, start, 0)),
            Some((best, at, since)) => {
                // the peak is the sound's start; 0.2 s past it (or well below it) it's settled
                if since >= 20 || score < best - 0.08 {
                    self.best = None;
                    if at > self.last_cue + 2000 {
                        self.last_cue = at;
                        return Some((at, best));
                    }
                } else {
                    self.best = Some((best, at, since + 1));
                }
            }
            None if score >= THRESHOLD => self.best = Some((score, start, 0)),
            None => {}
        }
        None
    }
}

fn game_pid(title: &str) -> u32 {
    let hwnd = crate::capture::find_game(title);
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return 0;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Listens to the game until nobody wants cues any more, the game goes away, or Windows stops the stream.
fn listen(shared: &Shared, listener: &mut Listener, pid: u32) -> Result<(), Box<dyn std::error::Error>> {
    use wasapi::*;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, RATE, 2, None);
    let mut client = AudioClient::new_application_loopback_client(pid, true)?;
    client.initialize_client(&fmt, &Direction::Capture, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 })?;
    let event = client.set_get_eventhandle()?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    shared.put_u32(32, 1);
    let mut bytes: VecDeque<u8> = VecDeque::new();
    let mut mono: Vec<f32> = Vec::new();
    let mut last_check = now_ms();
    loop {
        let now = now_ms();
        if !shared.wanted(now) {
            break;
        }
        if now > last_check + 2000 {
            last_check = now;
            if game_pid_alive(pid).is_none() {
                break;
            }
        }
        if event.wait_for_event(500).is_err() {
            continue; // the game is silent: no packets come
        }
        if capture.get_next_packet_size()?.unwrap_or(0) == 0 {
            continue;
        }
        capture.read_from_device_to_deque(&mut bytes)?;
        let end = now_ms();
        let (a, b) = bytes.as_slices();
        let raw: Vec<u8> = a.iter().chain(b).copied().collect();
        bytes.clear();
        mono.clear();
        for frame in raw.chunks_exact(8) {
            let l = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
            let r = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            mono.push((l + r) * 0.5);
        }
        shared.put_u64(40, end);
        if let Some((at, score)) = listener.feed(&mono, end) {
            shared.cue(at, score);
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

fn game_pid_alive(pid: u32) -> Option<()> {
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == 259; // STILL_ACTIVE
        CloseHandle(h);
        ok.then_some(())
    }
}

/// Starts the audio cue thread. `title` is the game window's title, as for the shared capture.
pub fn start(title: String) {
    std::thread::Builder::new()
        .name("audio-cues".into())
        .spawn(move || {
            let Some(shared) = Shared::create() else {
                eprintln!("audio cues: could not create {NAME}");
                return;
            };
            if wasapi::initialize_mta().ok().is_err() {
                shared.put_u32(32, 2);
                return;
            }
            let mut listener = Listener::new();
            loop {
                if !shared.wanted(now_ms()) {
                    shared.put_u32(32, 0);
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
                let pid = game_pid(&title);
                shared.put_u32(36, pid);
                if pid == 0 {
                    shared.put_u32(32, 2);
                    std::thread::sleep(Duration::from_millis(1000));
                    continue;
                }
                listener.reset();
                if let Err(e) = listen(&shared, &mut listener, pid) {
                    eprintln!("audio cues: {e}");
                    shared.put_u32(32, 2);
                    std::thread::sleep(Duration::from_millis(2000));
                }
            }
        })
        .ok();
}
