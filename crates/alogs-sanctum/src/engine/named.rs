//! The layout from the game's own log. The game names the Sanctum's scene as it loads it (200NN is Layout NN), so the
//! layout is known at once: nothing to work out from the minimap, and no wrong guess. With no name (it's a warning
//! the game logs, a patch could take it away) the minimap decides as before. A scene no layout here has (one new in
//! the game, or a layouts.json older than the game) is none of ours: nothing is matched to it, and the map says so.
//! Engine.cs's log-layout and log-candidates, as normal behaviour.

use super::{Core, GAME_LOG_SOURCE, Place};
use crate::layout_db::Layout;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

/// How long the candidates' maps wait for the log's name before they're loaded.
pub const NAME_WAIT: Duration = Duration::from_millis(1500);
/// A name this fresh (seconds): the player is still at the door gone in by.
pub const AT_THE_DOOR_S: f64 = 15.0;
/// What a load of candidates is told when no layout is wanted any more (a new run): no layout has this number.
const NONE_WANTED: i64 = -1;

impl Core {
    /// Takes the layout the log names for the Sanctum the player is in. Looked at after every read of the log, with or
    /// without a picture of the game.
    pub fn layout_from_log(&mut self) {
        if self.reset_requested || self.settings().forced_layout != 0 {
            return;
        }
        let Some((scene, age)) = self.game_log.named_sanctum() else {
            self.unknown_scene = None;
            // a line names a Sanctum scene in a wording this doesn't read: said once, to learn the game's new wording
            if let Some(m) = self.game_log.sanctum_mention.clone().filter(|m| self.unread_mention.as_ref() != Some(m)) {
                crate::log::write(&format!("game log: a Sanctum scene named in a wording not read as the layout: {m}"));
                self.unread_mention = Some(m);
            }
            return;
        };
        let Some(told) = self.db.by_scene(scene).cloned() else {
            if self.unknown_scene != Some(scene) {
                self.unknown_scene = Some(scene);
                crate::log::write(&format!("game log: the Sanctum is scene {scene}, which no layout here has: no map for it, and no other layout is matched"));
                self.end_preload(None);
                self.clear_guesses();
                if let Some(c) = self.current_layout.take() {
                    crate::log::write(&format!("game log: so it isn't {} (via {})", c.title(), self.layout_source));
                    self.layout_source.clear();
                    self.last_pose = None;
                    self.player = None;
                    self.minimap.reset();
                    {
                        let mut s = self.settings_mut();
                        s.run_layout = 0;
                        s.save();
                    }
                    self.sh.emit_status();
                }
            }
            return;
        };
        if self.current_layout.as_ref().is_some_and(|c| c.number == told.number) {
            // found from pictures first, or kept from before a restart, and the log agrees: nothing to look for again
            if self.layout_source != GAME_LOG_SOURCE {
                crate::log::write(&format!("game log: names Layout {} too (it came via {})", told.number, self.layout_source));
                self.layout_source = GAME_LOG_SOURCE.into();
                self.sh.emit_status();
            }
            return;
        }
        self.lock_layout(&told, GAME_LOG_SOURCE);
        self.clear_guesses();
        self.last_pose = None;
        self.player = None;
        self.minimap.reset();
        // just loaded: at the door gone in by. Named long ago (the helper started mid-run): the whole map is searched.
        if age < AT_THE_DOOR_S {
            let e = self.sanctum_entry(&told);
            self.minimap.seed(e);
        }
    }

    /// The Sanctum scene the log names when no layout here has it: the player is in a layout none of ours is, so the
    /// minimap and the full map aren't matched to any (a near one would be shown as if it were right).
    pub fn unmapped_scene(&self) -> Option<i64> {
        if self.settings().forced_layout != 0 {
            return None;
        }
        let (scene, _) = self.game_log.named_sanctum()?;
        self.db.by_scene(scene).is_none().then_some(scene)
    }

    /// Into a Sanctum with its layout unknown: the candidates' map art is loaded in the background, once the log has
    /// had its moment to name the one layout that's needed.
    pub fn preload_when_due(&mut self) {
        if self.preload_due.is_none_or(|t| Instant::now() < t) {
            return;
        }
        self.preload_due = None;
        if self.current_layout.is_some() || self.settings().forced_layout != 0 || self.game_log_place() != Place::Sanctum || self.unmapped_scene().is_some() {
            return;
        }
        let cands: Vec<Layout> = self.db.candidates(self.effective_difficulty().as_deref()).into_iter().cloned().collect();
        crate::log::write(&format!("game log: no name for this Sanctum after {:.1} s: loading {} candidates' maps", NAME_WAIT.as_secs_f64(), cands.len()));
        let arts = self.arts.clone();
        let known = Arc::new(AtomicI64::new(0));
        self.preload_known = Some(known.clone());
        std::thread::spawn(move || {
            for l in &cands {
                if known.load(Ordering::SeqCst) != 0 {
                    break;
                }
                arts.get(l, 0);
            }
            // the layout became known while a map was loading: that map came after the others were let go
            let k = known.load(Ordering::SeqCst);
            if k != 0 {
                arts.trim(|n| n == k);
            }
        });
    }

    /// The layout is known (or none is wanted any more): a load of candidates stops and keeps only that one.
    pub fn end_preload(&mut self, layout: Option<i64>) {
        self.preload_due = None;
        if let Some(k) = self.preload_known.take() {
            k.store(layout.unwrap_or(NONE_WANTED), Ordering::SeqCst);
        }
    }
}
