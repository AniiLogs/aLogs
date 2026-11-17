//! The layout from the minimap (standard; the full map is the fallback). Every run starts at the orange door, so each
//! candidate layout gets a tracker seeded at its orange door; the right one keeps matching as the player explores.
//! Rounds run in the background. Port of Engine.cs's ClearGuesses and GuessLayout.

use super::Core;
use crate::cv::Mat;
use crate::layout_db::Layout;
use crate::vision::MinimapTracker;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub type Guess = Arc<Mutex<(MinimapTracker, Vec<f64>)>>;

#[derive(Default)]
pub struct Guesses {
    pub map: BTreeMap<i64, Guess>,
    pub task: Option<JoinHandle<(i64, Vec<i64>)>>,
    next: Option<Instant>,
    started: Option<Instant>,
    whole_search: bool,
    widened: bool,
}

fn avg(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}

impl Core {
    /// True while the layout is being worked out from the minimap.
    pub fn identifying_layout(&self) -> bool {
        self.current_layout.is_none() && !self.guesses.is_empty()
    }

    pub fn clear_guesses(&mut self) {
        let had = !self.guesses.is_empty();
        self.guesses.map.clear();
        self.guesses.task = None;
        if had {
            self.sh.emit_status();
        }
    }

    fn new_guess(&self, l: &Layout) -> Guess {
        let mut t = MinimapTracker::new(self.arts.clone());
        if let Some(o) = l.orange() {
            t.seed((o.img[0], o.img[1]));
        }
        Arc::new(Mutex::new((t, vec![])))
    }

    pub fn guess_layout(&mut self, mm: &Mat, ui: f64, log_now: bool) {
        if let Some(task) = &self.guesses.task {
            if !task.is_finished() {
                return;
            }
            let (won, prune) = match self.guesses.task.take().unwrap().join() {
                Ok(r) => r,
                Err(_) => {
                    crate::log::write("guess: a round failed");
                    (0, vec![])
                }
            };
            for n in prune {
                self.guesses.map.remove(&n);
            }
            if won != 0
                && self.current_layout.is_none()
                && let Some(winner) = self.db.by_number(won).cloned()
            {
                let spot = self.guesses.map.get(&won).and_then(|g| g.lock().unwrap().0.last_position());
                let took = self.guesses.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
                crate::log::write(&format!("guess: {} after {took:.1} s", winner.title()));
                self.lock_layout(&winner, "minimap");
                // hand over to the main tracker from where the guess has the player now
                self.minimap.reset();
                let start = spot.or_else(|| winner.orange().map(|o| (o.img[0], o.img[1]))).unwrap_or((0.0, 0.0));
                self.minimap.seed(start);
                self.clear_guesses();
                return;
            }
        }
        if self.guesses.next.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.guesses.next = Some(Instant::now() + Duration::from_millis(600));
        let all: Vec<Layout> = self.db.candidates(None).into_iter().cloned().collect();
        let mut cands: Vec<Layout> = self.db.candidates(self.effective_difficulty().as_deref()).into_iter().cloned().collect();
        if cands.is_empty() {
            cands = all.clone();
        }
        if self.guesses.map.is_empty() {
            self.guesses.started = Some(Instant::now());
            self.guesses.whole_search = false;
            self.guesses.widened = false;
            crate::log::write(&format!("guess: identifying the layout from the minimap ({} candidates)", cands.len()));
            for l in &cands {
                let g = self.new_guess(l);
                self.guesses.map.insert(l.number, g);
            }
            self.sh.emit_status();
        }
        let elapsed = self.guesses.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
        let poor = |m: &BTreeMap<i64, Guess>| {
            m.values().all(|g| {
                let s = &g.lock().unwrap().1;
                s.len() >= 4 && avg(s) < 0.45
            })
        };
        // none of this difficulty's layouts fits after 10 s: the difficulty reading may be stale, add every layout
        if !self.guesses.widened && cands.len() < all.len() && elapsed > 10.0 && poor(&self.guesses.map) {
            self.guesses.widened = true;
            self.difficulty_doubted = true;
            crate::log::write(&format!("guess: no {} layout fits, trying every layout", self.effective_difficulty().unwrap_or_default()));
        }
        if self.difficulty_doubted {
            self.guesses.widened = true;
        }
        if self.guesses.widened {
            for l in &all {
                if !self.guesses.map.contains_key(&l.number) {
                    let g = self.new_guess(l);
                    self.guesses.map.insert(l.number, g);
                }
            }
            cands = all;
        }
        let live: Vec<(i64, Layout, Guess)> = self.guesses.map.iter().filter_map(|(n, g)| cands.iter().find(|c| c.number == *n).map(|l| (*n, l.clone(), g.clone()))).collect();
        if live.is_empty() {
            self.clear_guesses(); // difficulty changed: start over next round
            return;
        }
        // nobody fits after 20 s: the right layout averages 0.55+ within a few rounds from the orange door
        let restart = !self.guesses.whole_search
            && elapsed > 20.0
            && live.iter().all(|g| {
                let s = &g.2.lock().unwrap().1;
                s.len() >= 4 && avg(s) < 0.45
            });
        if restart {
            self.guesses.started = Some(Instant::now());
            self.guesses.whole_search = true;
        }
        // whole-layout searches cost ~0.25 s per layout: slower rounds
        if self.guesses.whole_search {
            self.guesses.next = Some(Instant::now() + Duration::from_millis(2500));
        }
        let shot = mm.clone();
        self.guesses.task = Some(std::thread::spawn(move || {
            if restart {
                // the player had left the spawn room before tracking began: every tracker searches its whole layout
                for g in &live {
                    let mut g = g.2.lock().unwrap();
                    g.0.reset();
                    g.1.clear();
                }
                crate::log::write("guess: no layout fits from the orange door, searching whole layouts");
            }
            for (_, l, g) in &live {
                let mut g = g.lock().unwrap();
                g.0.locate(&shot, l, ui);
                // nothing to compare on this frame: it says nothing about the layout
                if g.0.last_info.starts_with("minimap: too little") {
                    return (0, vec![]);
                }
                let s = g.0.last_score;
                g.1.push(s);
                if g.1.len() > 8 {
                    g.1.remove(0);
                }
            }
            let mut ranked: Vec<(i64, f64)> = live
                .iter()
                .filter_map(|(n, _, g)| {
                    let s = &g.lock().unwrap().1;
                    (s.len() >= 4).then(|| (*n, avg(s)))
                })
                .collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            if ranked.is_empty() {
                return (0, vec![]);
            }
            if log_now {
                crate::log::write(&format!("guess: {}", ranked.iter().take(4).map(|x| format!("{}:{:.2}", x.0, x.1)).collect::<Vec<_>>().join("  ")));
            }
            // drop layouts that clearly don't fit, so rounds stay cheap with 30 candidates
            let prune = if ranked.len() > 3 { ranked.iter().skip(3).filter(|x| x.1 < 0.25 && ranked[0].1 - x.1 > 0.25).map(|x| x.0).collect() } else { vec![] };
            let second = ranked.get(1).map_or(0.0, |x| x.1);
            (if ranked[0].1 >= 0.55 && ranked[0].1 - second >= 0.1 { ranked[0].0 } else { 0 }, prune)
        }));
    }
}

impl Guesses {
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
