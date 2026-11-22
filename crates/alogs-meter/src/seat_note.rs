//! "I was in this run": an Interlink fight the meter followed to its end whose result screen it never read (the player
//! left that screen before aLogs saw it) is never sent as a run, so the player's seat in a team-mate's copy stays
//! unlinked. A small note then goes instead, so the site can link the seat (kind interlink_presence: the run's own
//! fields, extra.instanceId, and the player's own seat and name when known). No score, no players, no pictures, no
//! input hold, no notice.

use crate::meter::Meter;
use alogs_pyval::{Dict, Value};

/// How long after the fight's end a result may still be recorded for it before the note goes: the result screen
/// comes 1.5 to 5 s after a fight's end and its first read records it at once, so a result not recorded by then never
/// will be (the player has left the screen).
pub const SEAT_NOTE_WAIT_S: f64 = 30.0;

/// The note's state.
#[derive(Default)]
pub struct SeatNoteState {
    /// The followed fight's instance (its space when it started), for its end.
    pub instance: Option<String>,
    /// A fight that ran to its end: (when it ended, the results recorded by then (ps.results), its note, its fight's
    /// name), sent SEAT_NOTE_WAIT_S on unless a result is recorded meanwhile.
    pub pending: Option<(f64, u64, Value, String)>,
    /// The fights a note went for (never twice for one).
    pub sent: Vec<String>,
}

impl Meter {
    /// A fight starts: an Interlink fight followed live with submissions on remembers its instance.
    pub fn seat_note_start(&mut self, bossrush: bool, submits: bool) {
        let instance = self.space.clone().filter(|_| bossrush && submits && !self.args.replay && alogs_gamelog::scene_of(self.space.as_deref()).is_none());
        self.seat_note.instance = instance;
        self.seat_note.pending = None;
    }

    /// The fight is over (`ending`: it was in its last seconds or on its last bar, nearly empty, as fight_ending says;
    /// `defeated`: its boss went down): its note waits for its result.
    pub fn seat_note_end(&mut self, f: &crate::fights::Fight, ending: bool, now: f64) {
        let Some(instance) = self.seat_note.instance.take() else { return };
        if !f.bossrush || !(ending || f.defeated) || self.seat_note.sent.contains(&f.csv_name) {
            if f.bossrush && !(ending || f.defeated) {
                log::info!(target: "meter", "seat note: none for {} (left before its end)", f.csv_name);
            }
            return;
        }
        let run = self.run_of(f);
        let mut s = Dict::new();
        for (k, v) in [
            ("boss", Value::Str(run.title)),
            ("puppetId", Value::Str(run.id)),
            ("level", Value::Int(f.level)),
            ("uid", self.uid.clone().map_or(Value::None, Value::Str)),
            ("cycle", self.env.schedule.active_cycle_id(now).map_or(Value::None, Value::Str)),
            ("started", Value::Str(crate::results::local(f.t0, "%Y-%m-%d %H:%M:%S"))),
            ("instance", Value::Str(instance)),
            ("server", crate::submit::game_server().map_or(Value::None, Value::Str)),
        ] {
            s.insert(k.into(), v);
        }
        if let Some(g) = self.game_build_now() {
            s.insert("game".into(), g);
        }
        // your own seat: from this run's Party read, only when it has an entry marked as you
        let slot = self.party_for_result(now, true).and_then(|p| p.iter().find(|m| m.get("self").truthy()).and_then(|m| m.get("slot").as_i64()));
        let uid = self.uid.clone().map_or(Value::None, Value::Str);
        let clock = self.env.record_clock();
        let note = crate::payloads::presence_payload(&Value::Dict(s), &f.csv_name, &uid, slot, self.character_name.as_deref(), &clock);
        self.seat_note.pending = Some((now, self.ps.results, note, f.csv_name.clone()));
    }

    /// Every look: a pending note goes once SEAT_NOTE_WAIT_S has passed with no result recorded for its fight; a result
    /// recorded meanwhile drops it (the run itself is sent). What decides is the fight's run file: a result in it, or
    /// the run taken up for sending or sent, means no note. (A result counted since the fight ended is a shortcut.)
    pub fn seat_note_check(&mut self, now: f64) {
        let Some((ended, results, note, name)) = self.seat_note.pending.clone() else { return };
        if self.ps.results > results {
            self.seat_note.pending = None;
            return;
        }
        if now - ended < SEAT_NOTE_WAIT_S {
            return;
        }
        self.seat_note.pending = None;
        self.seat_note.sent.push(name.clone());
        if self.run_has_result(&name) {
            log::info!(target: "meter", "seat note: none for {name} (its result was read)");
            return;
        }
        let Some(sub) = self.submitter.clone() else { return };
        log::info!(target: "meter", "seat note: {name} ran to its end but no result screen was read - telling the site you were in it");
        sub.enqueue(&note, None, None, None, None);
    }
}
