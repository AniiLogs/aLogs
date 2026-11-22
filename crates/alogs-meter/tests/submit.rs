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

/// An update with the fight's evidence: its pictures as fight<n> (JPEG, as they are) and its log as fightlog
/// (application/gzip), only beside the result screen; once sent, their files are gone with the other proof.
#[test]
fn fight_evidence_goes_with_the_result_screen() {
    let (api, seen) = server(vec![200, 200]);
    let d = setup(&api);
    std::fs::create_dir_all(d.join("logs/screens")).unwrap();
    let jpg = |n: u8| {
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 80).encode(&[n; 16 * 8 * 3], 16, 8, jpeg_encoder::ColorType::Rgb).unwrap();
        out
    };
    std::fs::write(d.join("logs/screens/fight_x_1.jpg"), jpg(1)).unwrap();
    std::fs::write(d.join("logs/screens/fight_x_4.jpg"), jpg(4)).unwrap();
    std::fs::write(d.join("logs/screens/fightlog_x.csv.gz"), b"\x1f\x8bmade-up").unwrap();
    let s = Submitter::new(d.clone());
    let runner = s.clone();
    std::thread::spawn(move || runner.run());
    let fight = vec![(1, "logs/screens/fight_x_1.jpg".to_string()), (4, "logs/screens/fight_x_4.jpg".to_string())];
    // without the result screen nothing of it goes (the site takes it only beside one)
    s.enqueue_with_fight(&payload(), None, None, None, None, &fight, Some("logs/screens/fightlog_x.csv.gz"));
    wait_for(|| seen.lock().unwrap().len() >= 1 && s.pending() == 0);
    {
        let got = seen.lock().unwrap();
        let body = String::from_utf8_lossy(&got[0].1);
        assert!(!body.contains("fight1") && !body.contains("fightlog"), "no evidence part without the result screen");
    }
    for f in ["logs/screens/fight_x_1.jpg", "logs/screens/fight_x_4.jpg"] {
        std::fs::write(d.join(f), jpg(f.as_bytes()[21])).unwrap();
    }
    std::fs::write(d.join("logs/screens/fightlog_x.csv.gz"), b"\x1f\x8bmade-up").unwrap();
    s.enqueue_with_fight(&payload(), Some("shot.png"), None, None, None, &fight, Some("logs/screens/fightlog_x.csv.gz"));
    let q = std::fs::read_to_string(d.join("logs/submit_queue/interlink_run__run1.json")).unwrap();
    assert!(q.contains("\"fight\"") && q.contains("fight_x_4.jpg") && q.contains("\"fightlog\""), "{q}");
    wait_for(|| seen.lock().unwrap().len() >= 2 && s.pending() == 0);
    {
        let got = seen.lock().unwrap();
        let body = &got[1].1;
        let text = String::from_utf8_lossy(body);
        assert!(text.contains("name=\"evidence\"") && text.contains("name=\"fight1\"; filename=\"fight1.jpg\"\r\nContent-Type: image/jpeg"), "{}", &text[..300.min(text.len())]);
        assert!(text.contains("name=\"fight4\"") && !text.contains("name=\"fight2\""));
        assert!(text.contains("name=\"fightlog\"; filename=\"fightlog.csv.gz\"\r\nContent-Type: application/gzip\r\n\r\n\u{1f}\u{fffd}made-up"));
        let want = jpg(b'4');
        assert!(body.windows(want.len()).any(|w| w == want.as_slice()), "the picture goes as it is");
    }
    for f in ["logs/screens/fight_x_1.jpg", "logs/screens/fight_x_4.jpg", "logs/screens/fightlog_x.csv.gz"] {
        assert!(!d.join(f).exists(), "{f} removed once sent");
    }
    s.stop.store(true, std::sync::atomic::Ordering::Relaxed);
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
