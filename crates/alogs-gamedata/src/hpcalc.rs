//! HP formulas decoded from formula_data.lua (Aniimo client). Port of aLogs B16 `bossbar/hpcalc.py`.
//!
//! The multiplier flag applies when it equals 1. Game build 3846247 (2026-10-29) raised the level cap: most curves
//! gained a piece above level 65 (3101 also rises only 40% as fast from 55), base HP (3003) grows faster above 65,
//! and 31331 is new.
//!
//! Every expression keeps the Python's operand order so results are bit-for-bit the same (tests/hpcalc.rs checks this
//! against goldens written by the reference).

use serde_json::Value;

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    // Python's max(lo, min(x, hi))
    lo.max(x.min(hi))
}

/// Python's `max(0.01, x)`.
fn floor01(x: f64) -> f64 {
    if x > 0.01 { x } else { 0.01 }
}

/// The level curve 3101 and 3133 share up to level 65: a cubic to 20, a quintic after.
fn curve(l: f64, c: &[f64; 10]) -> f64 {
    if l > 20.0 {
        c[3] * l.powi(5) - c[4] * l.powi(4) + c[5] * l.powi(3) - c[6] * l.powi(2) + c[7] * l - c[8]
    } else {
        c[0] * l.powi(3) - c[1] * l.powi(2) - c[2] * l + c[9]
    }
}

const C3101: [f64; 10] = [2.03e-06, 2.78e-06, 0.00025339, 5.03e-09, 1.4262e-06, 0.0001494542, 0.0071018605, 0.1568861725, 1.2146712941, 0.07617853];
const C3133: [f64; 10] = [5.5e-07, 3.46e-06, 3.547e-05, 7.49e-09, 1.9723e-06, 0.0001942808, 0.0088101791, 0.1884256915, 1.4456301243, 0.08198255];

fn flag_mult(flag: f64, m: f64) -> f64 {
    if flag == 1.0 { m } else { 1.0 }
}

fn h3101(l: f64) -> f64 {
    if l >= 66.0 {
        return 5.616e-05 * l.powi(3) - 0.0125772 * l.powi(2) + 0.94301915 * l - 23.15482319;
    }
    curve(l, &C3101)
}

fn f3101(lv: f64, a1: f64, a2: f64, a3: f64, flag: f64) -> f64 {
    let l = clamp(lv, 1.0, 80.0);
    let m = flag_mult(flag, 1.6);
    let mut f = h3101(l);
    if (55.0..=80.0).contains(&l) {
        let base = h3101(55.0);
        f = base + (f - base) * 0.4; // from 55 up it rises 40% as fast
    }
    floor01(f * a1 * a3 * a2 * m)
}

fn f3133(lv: f64, a1: f64, a2: f64, a3: f64, flag: f64) -> f64 {
    let l = clamp(lv, 1.0, 80.0);
    let m = flag_mult(flag, 1.6);
    let f = if l <= 65.0 {
        curve(l, &C3133)
    } else if l < 70.0 {
        let t = l - 65.0;
        0.4171869195 + 0.0064772012 * t - 0.0026404692 * t * t + 0.0003588403 * t * t * t
    } else {
        -3.22e-06 * l.powi(3) + 0.0004966 * l.powi(2) - 0.01502165 * l + 0.15105173
    };
    floor01(f * a1 * a3 * a2 * m)
}

fn f31331(lv: f64, a1: f64, a2: f64, a3: f64, flag: f64) -> f64 {
    let l = clamp(lv, 1.0, 70.0);
    let m = flag_mult(flag, 1.6);
    let f = if l < 61.0 {
        curve(l, &C3133)
    } else {
        let t = (l - 60.0) / 10.0;
        0.3711354057 - 0.0399616524 * t - 0.0088565031 * t * t - 0.0030902463 * t * t * t
    };
    floor01(f * a1 * a3 * a2 * m)
}

fn quad(lv: f64, a1: f64, flag: f64, q2: f64, q1: f64, q0: f64, div: f64) -> f64 {
    let l = clamp(lv, 1.0, 60.0);
    let m = flag_mult(flag, 1.6);
    floor01((q2 * l * l - q1 * l + q0) * a1 / div * m / 1.6)
}

fn f3046(lv: f64, a1: f64, flag: f64) -> f64 {
    let l = clamp(lv, 1.0, 60.0);
    let m = flag_mult(flag, 1.6);
    floor01(
        (-6.776e-06 * l.powi(4) + 0.0008627 * l.powi(3) + 0.00089 * l.powi(2) - 0.08677 * l + 4.556) * a1 / 90.0 * m
            / 1.6,
    )
}

fn f31xx(lv: f64, a1: f64, flag: f64, c: &[f64; 9], high: &[f64; 4]) -> f64 {
    let l = clamp(lv, 1.0, 80.0);
    let m = flag_mult(flag, 1.25);
    let f = if l >= 66.0 {
        high[0] * l.powi(3) + high[1] * l.powi(2) + high[2] * l + high[3]
    } else if l > 20.0 {
        c[3] * l.powi(4) - c[4] * l.powi(3) + c[5] * l.powi(2) - c[6] * l + c[7]
    } else {
        c[0] * l.powi(3) - c[1] * l.powi(2) + c[2] * l + c[8]
    };
    floor01(f * a1 * m)
}

const C3111: [f64; 9] = [1.16e-06, 2.585e-05, 0.00015488, 1.4383e-07, 3.4322e-05, 0.0029873201, 0.0993568616, 2.1258581377, 1.03610667];
const C3123: [f64; 9] = [1.06e-06, 2.35e-05, 0.0001408, 2.0213e-07, 5.60311e-05, 0.005153242, 0.1737310687, 2.9376494859, 1.03464242];
const H3111: [f64; 4] = [-3.819e-05, 0.00891099, -0.67669559, 18.30807191]; // from level 66 (build 3846247)
const H3123: [f64; 4] = [5.844e-05, -0.01332508, 1.01180489, -23.86934159];

fn f3016(lv: f64, a: f64, b: f64, c: f64) -> f64 {
    let r = if lv <= 25.0 { a + (b - a) / 10.0 * (lv - 15.0) } else { b + (c - b) / 25.0 * (lv - 25.0) };
    a.max(c.min(r))
}

/// A number the way hpcalc.rate takes it: ints, floats and bools count, anything else is 0.
fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// `entry` is property_data's `fixed_hp_rate` object `{"1": formulaId, "2": arg1, ...}`. Returns the HP multiplier,
/// or `None` for an unknown formula or an entry that isn't an object.
pub fn rate(entry: &Value, lv: f64) -> Option<f64> {
    let obj = entry.as_object()?;
    let f = match obj.get("1") {
        Some(Value::Number(n)) => n.as_f64()?,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => return None,
    };
    let a: Vec<f64> = (2..7).map(|i| num(obj.get(&i.to_string()))).collect();
    Some(match f {
        3.0 => a[0],
        3016.0 => f3016(lv, a[0], a[1], a[2]),
        3101.0 => f3101(lv, a[0], a[1], a[2], a[3]),
        3133.0 => f3133(lv, a[0], a[1], a[2], a[3]),
        31331.0 => f31331(lv, a[0], a[1], a[2], a[3]),
        3041.0 => quad(lv, a[0], a[1], 0.005634, 0.05541, 1.664, 25.0),
        3036.0 => quad(lv, a[0], a[1], 0.005634, 0.05541, 1.664, 15.0),
        3031.0 => quad(lv, a[0], a[1], 0.003978, 0.03911, 1.178, 10.0),
        3046.0 => f3046(lv, a[0], a[1]),
        3111.0 => f31xx(lv, a[0], a[1], &C3111, &H3111),
        3123.0 => f31xx(lv, a[0], a[1], &C3123, &H3123),
        _ => return None,
    })
}

/// Formula 3003 for a wild monster or boss (not a pet): from level 66 the level part grows faster.
pub fn total(species: f64, lv: i64, ind: f64) -> i64 {
    let extra = if lv >= 66 { (2 * (lv - 65)).min(lv - 60) } else { 0 };
    let lvf = lv as f64;
    ((species + 20.0) * (1.0 + 0.008 * ind) / 120.0 * ((6.0 * lvf + 35.0) + (extra * 6) as f64 * 0.3) * 20.0).floor()
        as i64
}

/// Max HP at a level: base HP (3003) times the `fixed_hp_rate` multiplier, rounded half to even like Python's
/// `round()`. `None` when the rate formula is unknown.
pub fn max_hp(species: f64, entry: &Value, lv: i64, ind: f64) -> Option<i64> {
    let r = rate(entry, lv as f64)?;
    Some((total(species, lv, ind) as f64 * r).round_ties_even() as i64)
}

/// Break gauge size (`bp_max_v`): formulas 3109/31091/31092 (boss rush bosses, flag 1 -> x1.6), 3050/3051 and 3052
/// (overworld bosses). Missing args count as 0.
pub fn break_points(formula: i64, args: &[f64], level: f64) -> Option<f64> {
    let g = |i: usize| args.get(i).copied().unwrap_or(0.0);
    let (a, b, c) = (g(0), g(1), g(2));
    let late = |l: f64| {
        1.0 - 0.08 * clamp(l - 29.0, 0.0, 1.0) - 0.08 * clamp(l - 39.0, 0.0, 1.0) - 0.08 * clamp(l - 49.0, 0.0, 1.0)
    };
    let low = |l: f64| 0.03248729 * l.powi(3) - 0.84002093 * l.powi(2) + 42.04376314 * l + 281.42637984;
    let mid = |l: f64| {
        0.0001463926 * l.powi(4) - 0.0611009251 * l.powi(3) + 7.0542305125 * l.powi(2) - 236.9426417667 * l
            + 3540.3670946658
    };
    let high = |l: f64| 0.22539909 * l.powi(3) - 49.20816395 * l.powi(2) + 3647.82104969 * l - 86891.32460022;
    match formula {
        3 => Some(a),
        3109 => {
            let l = clamp(level, 1.0, 80.0);
            let m = flag_mult(c, 1.6);
            let f = if l <= 20.0 { low(l) } else if l < 66.0 { mid(l) } else { high(l) };
            Some(floor01(f * a * b * m * late(l)))
        }
        31091 | 31092 => {
            // build 3846247: boss rush bosses' new gauges
            let l = clamp(level, 1.0, 80.0);
            let m = flag_mult(c, 1.6);
            let top = if formula == 31091 { 66.0 } else { 61.0 };
            let f = if l <= 20.0 {
                low(l)
            } else if l < top {
                mid(l)
            } else if formula == 31091 {
                high(l)
            } else {
                0.11806013 * l.powi(3) - 26.92257602 * l.powi(2) + 2053.71135437 * l - 47947.98087692
            };
            if formula == 31091 {
                return Some(floor01(f * a * b * m)); // no late-level discount
            }
            Some(floor01(f * a * b * m * late(l)))
        }
        3050 | 3051 => {
            // open-world bosses (args: size, flag)
            let x = clamp(level, 1.0, 60.0);
            let m = flag_mult(b, 1.6);
            let f = if formula == 3050 {
                (-2.484e-05 * x.powi(4) + 0.002265 * x.powi(3) - 0.04807 * x.powi(2) + 0.5134 * x + 4.114) * a / 60.0
            } else {
                (-3.926e-05 * x.powi(3) + 0.005005 * x.powi(2) - 0.04969 * x + 1.234) * a / 15.0
            };
            Some(0.0f64.max(f * m / 1.6 * 10.0 * (4.0 * level + 35.0)))
        }
        3052 => {
            let x = clamp(a, 1.0, 60.0);
            Some(0.0f64.max(
                (7.885e-06 * x.powi(3) - 0.0007003 * x.powi(2) - 0.003136 * x + 1.486) * b * c * 10.0
                    * (4.0 * level + 35.0),
            ))
        }
        _ => None,
    }
}
