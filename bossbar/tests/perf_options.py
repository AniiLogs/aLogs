"""General Settings › Performance (app/ui/perf-options.js): every option the tab offers is in the meter's defaults
(settings.DEFAULTS['perf']) with the same default, so a settings save by the meter never drops it (its merge keeps only
keys it knows), and each option's default is one of its own choices.

    python tests/perf_options.py

Reads perf-options.js as text (no browser needed). Exit code 1 on any failure.
"""
import os, re, sys
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); sys.path.insert(0, HERE)
import settings as S

fails = []
src = open(os.path.join(HERE, '..', 'app', 'ui', 'perf-options.js'), encoding='utf-8').read()
options = re.findall(r'id: "(\w+)", plugin: "(\w+)", key: "([\w.]+)", default: ("?[\w.]+"?)(.*?)\n    \},', src, re.S)
if not options: fails.append('no options read from perf-options.js')
sections = re.findall(r'\["(\w+)", "[^"]+", "', src)
perf = S.DEFAULTS.get('perf', {})
if perf.get('on') is not False: fails.append(f"settings.py perf.on should default to False, is {perf.get('on')!r}")
for oid, plugin, key, default, rest in options:
    value = default.strip('"') if default.startswith('"') else float(default)
    if plugin not in sections: fails.append(f'{oid}: add-on {plugin!r} has no section')
    if oid not in perf: fails.append(f'{oid}: missing from settings.DEFAULTS["perf"]')
    elif perf[oid] != value: fails.append(f'{oid}: settings.py default {perf[oid]!r}, perf-options.js {value!r}')
    choices = re.findall(r'\[("?[\w.]+"?), "', rest)
    vals = [c.strip('"') if c.startswith('"') else float(c) for c in choices]
    if value not in vals: fails.append(f'{oid}: default {value!r} is not one of its choices {vals}')
    group, _, last = key.partition('.')
    if last not in S.DEFAULTS.get(group, {}): fails.append(f'{oid}: its setting {key} is not a meter setting')
extra = set(perf) - {'on'} - {o[0] for o in options}
if extra: fails.append(f'settings.py has perf options the tab does not: {sorted(extra)}')
print(f'{len(options)} options in {len(sections)} sections')
if fails:
    print('FAIL\n  ' + '\n  '.join(fails)); sys.exit(1)
print('PASS')
