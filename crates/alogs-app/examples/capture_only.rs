// Runs only the shared capture (for testing readers without the whole app): cargo run --example capture_only
#[path = "../src/capture.rs"]
mod capture;

fn main() {
    capture::start(std::env::args().nth(1).unwrap_or_else(|| "Aniimo".into()));
    std::thread::sleep(std::time::Duration::from_secs(std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60)));
}
