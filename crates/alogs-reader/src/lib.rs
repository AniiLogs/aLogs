//! Reading the game's screen: the boss bar, its fill and break gauge, the texts around it and the xN counter.
//! Ports of aLogs B16 `bossbar/reader.py` and `xn_digits.py` (OCR itself is in `ocr`).

pub mod bar;
pub mod boss;
pub mod parse;
pub mod prep;
pub mod text;
pub mod xn;
