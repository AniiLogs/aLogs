# HP formulas decoded from formula_data.lua (Aniimo client). Flag semantics: multiplier applies when flag == 1.
# Game build 3846247 (2026-10-29) raised the level cap: most curves gained a piece above level 65 (3101 also rises
# only 40% as fast from 55), base HP (3003) grows faster above 65, and 31331 is new. Checked against the game's own
# bytecode at every level 1-80.
import math

def clamp(x, lo, hi): return max(lo, min(hi, x))

def _curve(L, c):
    """The level curve 3101 and 3133 share up to level 65: a cubic to 20, a quintic after."""
    if L > 20: return c[3]*L**5 - c[4]*L**4 + c[5]*L**3 - c[6]*L**2 + c[7]*L - c[8]
    return c[0]*L**3 - c[1]*L**2 - c[2]*L + c[9]

C3101 = (2.03e-06, 2.78e-06, 0.00025339, 5.03e-09, 1.4262e-06, 0.0001494542, 0.0071018605, 0.1568861725, 1.2146712941, 0.07617853)
C3133 = (5.5e-07, 3.46e-06, 3.547e-05, 7.49e-09, 1.9723e-06, 0.0001942808, 0.0088101791, 0.1884256915, 1.4456301243, 0.08198255)

def _h3101(L):
    if L >= 66: return 5.616e-05*L**3 - 0.0125772*L**2 + 0.94301915*L - 23.15482319
    return _curve(L, C3101)

def _3101(lv, a1, a2, a3, flag):
    L = clamp(lv, 1, 80); m = 1.6 if flag == 1 else 1
    f = _h3101(L)
    if 55 <= L <= 80:
        base = _h3101(55); f = base + (f - base) * 0.4          # from 55 up it rises 40% as fast
    return max(0.01, f * a1 * a3 * a2 * m)

def _3133(lv, a1, a2, a3, flag):
    L = clamp(lv, 1, 80); m = 1.6 if flag == 1 else 1
    if L <= 65: f = _curve(L, C3133)
    elif L < 70:
        t = L - 65; f = 0.4171869195 + 0.0064772012*t - 0.0026404692*t*t + 0.0003588403*t*t*t
    else: f = -3.22e-06*L**3 + 0.0004966*L**2 - 0.01502165*L + 0.15105173
    return max(0.01, f * a1 * a3 * a2 * m)

def _31331(lv, a1, a2, a3, flag):
    L = clamp(lv, 1, 70); m = 1.6 if flag == 1 else 1
    if L < 61: f = _curve(L, C3133)
    else:
        t = (L - 60) / 10; f = 0.3711354057 - 0.0399616524*t - 0.0088565031*t*t - 0.0030902463*t*t*t
    return max(0.01, f * a1 * a3 * a2 * m)

def _quad(lv, a1, flag, q2, q1, q0, div):
    L = clamp(lv, 1, 60); m = 1.6 if flag == 1 else 1
    return max(0.01, (q2*L*L - q1*L + q0) * a1 / div * m / 1.6)

def _3046(lv, a1, flag):
    L = clamp(lv, 1, 60); m = 1.6 if flag == 1 else 1
    return max(0.01, (-6.776e-06*L**4 + 0.0008627*L**3 + 0.00089*L**2 - 0.08677*L + 4.556) * a1 / 90 * m / 1.6)

def _31xx(lv, a1, flag, c, high):
    L = clamp(lv, 1, 80); m = 1.25 if flag == 1 else 1
    if L >= 66: f = high[0]*L**3 + high[1]*L**2 + high[2]*L + high[3]
    elif L > 20: f = c[3]*L**4 - c[4]*L**3 + c[5]*L**2 - c[6]*L + c[7]
    else: f = c[0]*L**3 - c[1]*L**2 + c[2]*L + c[8]
    return max(0.01, f * a1 * m)

C3111 = (1.16e-06, 2.585e-05, 0.00015488, 1.4383e-07, 3.4322e-05, 0.0029873201, 0.0993568616, 2.1258581377, 1.03610667)
C3123 = (1.06e-06, 2.35e-05, 0.0001408, 2.0213e-07, 5.60311e-05, 0.005153242, 0.1737310687, 2.9376494859, 1.03464242)
H3111 = (-3.819e-05, 0.00891099, -0.67669559, 18.30807191)        # from level 66 (build 3846247)
H3123 = (5.844e-05, -0.01332508, 1.01180489, -23.86934159)

def _3016(lv, a, b, c):
    r = a + (b - a) / 10 * (lv - 15) if lv <= 25 else b + (c - b) / 25 * (lv - 25)
    return max(a, min(c, r))

def rate(entry, lv):
    """entry = property_data fixed_hp_rate dict {'1': formulaId, '2': arg1, ...}; returns multiplier or None"""
    if not isinstance(entry, dict): return None
    f = entry.get('1'); a = [entry.get(str(i)) for i in range(2, 7)]
    a = [x if isinstance(x, (int, float)) else 0 for x in a]
    if f == 3: return a[0]
    if f == 3016: return _3016(lv, a[0], a[1], a[2])
    if f == 3101: return _3101(lv, a[0], a[1], a[2], a[3])
    if f == 3133: return _3133(lv, a[0], a[1], a[2], a[3])
    if f == 31331: return _31331(lv, a[0], a[1], a[2], a[3])
    if f == 3041: return _quad(lv, a[0], a[1], 0.005634, 0.05541, 1.664, 25)
    if f == 3036: return _quad(lv, a[0], a[1], 0.005634, 0.05541, 1.664, 15)
    if f == 3031: return _quad(lv, a[0], a[1], 0.003978, 0.03911, 1.178, 10)
    if f == 3046: return _3046(lv, a[0], a[1])
    if f == 3111: return _31xx(lv, a[0], a[1], C3111, H3111)
    if f == 3123: return _31xx(lv, a[0], a[1], C3123, H3123)
    return None

def total(species, lv, ind=0):
    """Formula 3003 for a wild monster or boss (not a pet): from level 66 the level part grows faster."""
    extra = min(2 * (lv - 65), lv - 60) if lv >= 66 else 0
    return math.floor((species + 20) * (1 + 0.008 * ind) / 120 * (6 * lv + 35 + extra * 6 * 0.3) * 20)

def max_hp(species, entry, lv, ind=0):
    r = rate(entry, lv)
    return None if r is None else round(total(species, lv, ind) * r)

# Break gauge size (bp_max_v) - formulas 3109 (boss rush bosses, flag 1 -> x1.6) and 3052 (overworld bosses), see
# aniimo_data/decoder/boss_break.py. Matches a level 60 Omega Sherro fight within 1%.
def break_points(formula, args, level):
    a, b, c = (list(args) + [0, 0, 0])[:3]
    if formula == 3: return a
    if formula == 3109:
        L = clamp(level, 1, 80); m = 1.6 if c == 1 else 1
        if L <= 20: f = 0.03248729 * L**3 - 0.84002093 * L**2 + 42.04376314 * L + 281.42637984
        elif L < 66: f = 0.0001463926 * L**4 - 0.0611009251 * L**3 + 7.0542305125 * L**2 - 236.9426417667 * L + 3540.3670946658
        else: f = 0.22539909 * L**3 - 49.20816395 * L**2 + 3647.82104969 * L - 86891.32460022
        late = 1 - 0.08 * clamp(L - 29, 0, 1) - 0.08 * clamp(L - 39, 0, 1) - 0.08 * clamp(L - 49, 0, 1)
        return max(0.01, f * a * b * m * late)
    if formula in (31091, 31092):                                   # build 3846247: boss rush bosses' new gauges
        L = clamp(level, 1, 80); m = 1.6 if c == 1 else 1
        top = 66 if formula == 31091 else 61
        if L <= 20: f = 0.03248729 * L**3 - 0.84002093 * L**2 + 42.04376314 * L + 281.42637984
        elif L < top: f = 0.0001463926 * L**4 - 0.0611009251 * L**3 + 7.0542305125 * L**2 - 236.9426417667 * L + 3540.3670946658
        elif formula == 31091: f = 0.22539909 * L**3 - 49.20816395 * L**2 + 3647.82104969 * L - 86891.32460022
        else: f = 0.11806013 * L**3 - 26.92257602 * L**2 + 2053.71135437 * L - 47947.98087692
        if formula == 31091: return max(0.01, f * a * b * m)      # no late-level discount
        late = 1 - 0.08 * clamp(L - 29, 0, 1) - 0.08 * clamp(L - 39, 0, 1) - 0.08 * clamp(L - 49, 0, 1)
        return max(0.01, f * a * b * m * late)
    if formula in (3050, 3051):                                     # open-world bosses (args: size, flag)
        x = clamp(level, 1, 60); m = 1.6 if b == 1 else 1
        if formula == 3050: f = (-2.484e-05 * x**4 + 0.002265 * x**3 - 0.04807 * x**2 + 0.5134 * x + 4.114) * a / 60
        else: f = (-3.926e-05 * x**3 + 0.005005 * x**2 - 0.04969 * x + 1.234) * a / 15
        return max(0.0, f * m / 1.6 * 10 * (4 * level + 35))
    if formula == 3052:
        x = clamp(a, 1, 60)
        return max(0.0, (7.885e-06 * x**3 - 0.0007003 * x**2 - 0.003136 * x + 1.486) * b * c * 10 * (4 * level + 35))
    return None
