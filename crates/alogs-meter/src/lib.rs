//! The meter's logic: following a fight on the boss bar, end screens into runs, the reading loop.
//! Ports of aLogs B16 `bossbar/fights.py`, `results.py`, `core.py` and `live.py`.

pub mod diag;
pub mod egg;
pub mod eggheist;
pub mod env;
pub mod fights;
pub mod hits;
pub mod housekeeping;
pub mod leaderboards;
pub mod menu;
pub mod meter;
pub mod ocrcheck;
pub mod overlay;
pub mod pacing;
pub mod party;
pub mod party_icons;
pub mod payloads;
pub mod results;
pub mod submit;
pub mod results_parse;
pub mod screens;
pub mod sim;
pub mod sim_tour;
pub mod team;
