//! Reads text crops on an OCR helper of its own, on its own thread, so the frame loop never waits for OCR (one job
//! at a time; the result is taken on a later look with what was kept alongside the job). Port of `OcrWorker` in aLogs
//! B16 `bossbar/core.py` (the meter's `ocr_bg` for the boss bar's text and `screen_bg` for the screens').

use alogs_pil::Image;
use alogs_reader::ocr::Reader;
use indexmap::IndexMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

type Crops = IndexMap<String, Image>;
type Text = IndexMap<String, String>;

struct State<M> {
    job: Option<(Crops, M)>,
    result: Option<(Text, M)>,
    /// The job being read is no longer wanted: its text is thrown away when it is done.
    unwanted: bool,
    stop: bool,
    restart: bool,
    longest_ms: f64,
}

pub struct OcrWorker<M: Send + 'static> {
    shared: Arc<(Mutex<State<M>>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl<M: Send + 'static> OcrWorker<M> {
    /// A worker whose helper `make` starts on its thread (so the meter's first look doesn't wait for it). With no
    /// helper (it didn't start) every job comes back with no text.
    pub fn new(name: &str, make: impl FnOnce() -> Option<Box<dyn Reader>> + Send + 'static) -> OcrWorker<M> {
        let shared = Arc::new((Mutex::new(State { job: None, result: None, unwanted: false, stop: false, restart: false, longest_ms: 0.0 }), Condvar::new()));
        let s2 = shared.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let ocr = make();
                if ocr.is_none() {
                    log::warn!(target: "meter", "background OCR helper did not start");
                }
                let (lock, cv) = &*s2;
                loop {
                    let job = {
                        let mut st = lock.lock().unwrap();
                        while st.job.is_none() && !st.stop && !st.restart {
                            st = cv.wait_timeout(st, Duration::from_millis(500)).unwrap().0;
                        }
                        if st.stop {
                            return;
                        }
                        if std::mem::take(&mut st.restart) {
                            drop(st);
                            if let Some(o) = &ocr
                                && let Err(e) = o.restart()
                            {
                                log::warn!(target: "meter", "background OCR helper did not restart: {e}");
                            }
                            continue;
                        }
                        let (crops, _) = st.job.as_ref().unwrap();
                        crops.clone()
                    };
                    let t0 = Instant::now();
                    let text = ocr.as_ref().map_or_else(IndexMap::new, |o| o.read(&job, &IndexMap::new(), &IndexMap::new()));
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let mut st = lock.lock().unwrap();
                    st.longest_ms = st.longest_ms.max(ms);
                    if let Some((_, meta)) = st.job.take()
                        && !std::mem::take(&mut st.unwanted)
                    {
                        st.result = Some((text, meta));
                    }
                }
            })
            .ok();
        OcrWorker { shared, thread }
    }

    /// A job is being read.
    pub fn busy(&self) -> bool {
        self.shared.0.lock().unwrap().job.is_some()
    }

    /// Read these crops; `meta` comes back with the text. Ignored while busy (one job at a time).
    pub fn submit(&self, crops: Crops, meta: M) {
        let mut st = self.shared.0.lock().unwrap();
        if st.job.is_none() {
            st.job = Some((crops, meta));
            self.shared.1.notify_all();
        }
    }

    /// The finished job's text and meta, once.
    pub fn take(&self) -> Option<(Text, M)> {
        self.shared.0.lock().unwrap().result.take()
    }

    /// What was read, and what is being read, is no longer wanted (it was of something else): no result comes of it.
    pub fn forget(&self) {
        let mut st = self.shared.0.lock().unwrap();
        st.result = None;
        st.unwanted = st.job.is_some();
    }

    /// Start its helper again (English OCR added while the meter runs), before its next job.
    pub fn restart(&self) {
        self.shared.0.lock().unwrap().restart = true;
        self.shared.1.notify_all();
    }

    /// The longest read since the last call (ms), for the load line.
    pub fn take_longest(&self) -> f64 {
        std::mem::take(&mut self.shared.0.lock().unwrap().longest_ms)
    }
}

impl<M: Send + 'static> Drop for OcrWorker<M> {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().stop = true;
        self.shared.1.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;
    impl Reader for Echo {
        fn read(&self, images: &IndexMap<String, Image>, _: &IndexMap<String, usize>, _: &IndexMap<String, Image>) -> IndexMap<String, String> {
            std::thread::sleep(Duration::from_millis(30));
            images.iter().map(|(k, v)| (k.clone(), format!("{}x{}", v.width, v.height))).collect()
        }
        fn cached(&self, _: &IndexMap<String, Image>, _: &IndexMap<String, usize>) -> bool {
            false
        }
        fn language(&self) -> Option<String> {
            None
        }
        fn started(&self) -> bool {
            true
        }
    }

    /// What was read of something else is no longer wanted: neither a result already waiting nor the job being read
    /// comes back, and the worker takes the next job as usual.
    #[test]
    fn a_forgotten_job_gives_no_result() {
        let w: OcrWorker<u32> = OcrWorker::new("test-forget", || Some(Box::new(Echo) as Box<dyn Reader>));
        let crops = || -> Crops { [("a".to_string(), Image::new(alogs_pil::Mode::L, 4, 2, &[0]))].into_iter().collect() };
        // a job being read
        w.submit(crops(), 1);
        w.forget();
        std::thread::sleep(Duration::from_millis(120));
        assert!(w.take().is_none(), "the job that was being read");
        assert!(!w.busy());
        // a result waiting
        w.submit(crops(), 2);
        std::thread::sleep(Duration::from_millis(120));
        w.forget();
        assert!(w.take().is_none(), "the result that was waiting");
        // the next job is read and comes back
        w.submit(crops(), 3);
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(w.take().map(|r| r.1), Some(3));
    }

    #[test]
    fn one_job_at_a_time_and_the_meta_comes_back() {
        let w: OcrWorker<u32> = OcrWorker::new("test-ocr", || Some(Box::new(Echo) as Box<dyn Reader>));
        let mut crops = IndexMap::new();
        crops.insert("a".to_string(), Image::new(alogs_pil::Mode::L, 3, 2, &[0]));
        w.submit(crops.clone(), 7);
        w.submit(crops, 8); // busy: ignored
        assert!(w.busy());
        let t = Instant::now();
        let got = loop {
            if let Some(r) = w.take() {
                break r;
            }
            assert!(t.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(got.1, 7);
        assert_eq!(got.0["a"], "3x2");
        assert!(!w.busy() && w.take().is_none());
        assert!(w.take_longest() >= 25.0);
    }
}
