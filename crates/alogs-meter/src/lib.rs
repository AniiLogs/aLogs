//! The meter's logic: following a fight on the boss bar, end screens into runs, the reading loop.
//! Ports of aLogs B16 `bossbar/fights.py`, `results.py`, `core.py` and `live.py`.

pub mod env;
pub mod fights;
pub mod payloads;
pub mod results_parse;
pub mod team;
