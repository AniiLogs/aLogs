//! The sender against a local stand-in for the leaderboard API: a queued record goes up as multipart with its proof
//! pictures and leaves the queue; a 400 sets it aside as .rejected; a 500 keeps it for later.

use alogs_meter::submit::Submitter;
use alogs_pil::{Image, Mode};
use alogs_pyval::{Value, dict};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn server(statuses: Vec<u16>) -> (String, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for (i, conn) in l.incoming().enumerate() {
            let mut c = conn.unwrap();
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            s2.lock().unwrap().push((line.trim().to_string(), body));
            let st = *statuses.get(i).unwrap_or(&200);
            let reply = if st == 200 { r#"{"submissionId": "s1"}"# } else { r#"{"error": "no"}"# };
            let _ = write!(c, "HTTP/1.1 {st} X\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{reply}", reply.len());
        }
    });
    (addr, seen)
}

fn setup(api: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("alogs-submit-{}", uuid_like()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("settings.json"), format!(r#"{{"leaderboard": {{"submit": true, "token": "alb_test", "api_url": "{api}"}}}}"#)).unwrap();
    let shot = Image::new(Mode::Rgb, 64, 36, &[10, 20, 30]);
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, 64, 36);
        e.set_color(png::ColorType::Rgb);
        e.write_header().unwrap().write_image_data(&shot.data).unwrap();
    }
    std::fs::write(d.join("shot.png"), png).unwrap();
    d
}

fn uuid_like() -> String {
    format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

fn payload() -> Value {
    dict([("kind", Value::Str("interlink_run".into())), ("clientRunId", Value::Str("run1".into())), ("players", Value::List(vec![])), ("server", Value::Str("x".into()))])
}

fn wait_for(cond: impl Fn() -> bool) {
    for _ in 0..100 {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn sends_and_sets_aside() {
    let (api, seen) = server(vec![200, 400, 500]);
    let d = setup(&api);
    let s = Submitter::new(d.clone());
    let runner = s.clone();
    std::thread::spawn(move || runner.run());
    s.enqueue(&payload(), Some("shot.png"), None, None, None);
    wait_for(|| s.pending() == 0);
    assert_eq!(s.pending(), 0, "sent and removed");
    {
        let got = seen.lock().unwrap();
        assert!(got[0].0.starts_with("POST /leaderboard/submissions"), "{}", got[0].0);
        let body = String::from_utf8_lossy(&got[0].1);
        assert!(body.contains("name=\"submission\"") && body.contains("\"clientRunId\": \"run1\"") && body.contains("name=\"evidence\""), "multipart body");
        assert!(body.contains("\"proofBoxes\""), "proof boxes go with an interlink run");
    }
    // a 400: set aside
    let p2 = dict([("kind", Value::Str("sim_run".into())), ("clientRunId", Value::Str("bad".into())), ("server", Value::Str("x".into()))]);
    s.enqueue(&p2, None, None, None, None);
    wait_for(|| d.join("logs/submit_queue/sim_run__bad.json.rejected").exists());
    assert!(d.join("logs/submit_queue/sim_run__bad.json.rejected").exists());
    // a 500: kept for later
    let p3 = dict([("kind", Value::Str("sim_run".into())), ("clientRunId", Value::Str("later".into())), ("server", Value::Str("x".into()))]);
    s.enqueue(&p3, None, None, None, None);
    wait_for(|| seen.lock().unwrap().len() >= 3);
    std::thread::sleep(Duration::from_millis(200));
    assert!(d.join("logs/submit_queue/sim_run__later.json").exists(), "kept after a 500");
    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
}
