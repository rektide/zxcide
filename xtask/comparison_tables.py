"""Schema 1 job definitions, evidence checks and pure Markdown renderer."""
import argparse
import json
from pathlib import Path
from statistics import median

from scorecard import FILES

MODES = {'split': 'C3', 'dense': 'C7'}
QUICK = ('dickens', 'sao')
SAMPLES = 7
SEED = 42


def levels_arg(text):
    values = text.split(',')
    if any(s not in tuple('1234567') for s in values) or len(set(values)) != len(values):
        raise argparse.ArgumentTypeError('levels must be distinct integers 1..7 separated by commas')
    return list(map(int, values))


def check(ok, message):
    if not ok:
        raise ValueError(message)


def rows(raw, kind):
    points = [f'C{n}' for n in raw['levels']]
    for point in points:
        if kind == 'decode':
            for impl in raw['binaries']:
                yield point, impl
        else:
            yield point, 'c'
            for mode, pair in MODES.items():
                if pair == point:
                    yield from ((mode, i) for i in raw['binaries'] if i != 'c')
    if kind == 'encode':
        for mode, pair in MODES.items():
            if pair not in points:
                yield from ((mode, i) for i in raw['binaries'] if i != 'c')


def archive_key(kind, point, impl):
    return f'{impl if kind == "encode" else "c"}.{point}'


def decode_argv(impl, path):
    return [f'bin/{impl}', *(['-qdc', '-T1'] if impl == 'c' else ['decode']), path]


def encode_argv(impl, point, name):
    args = ['-qzc', '-T1', f'-{point[1:]}'] if impl == 'c' else ['encode', '--mode', point]
    return [f'bin/{impl}', *args, f'scratch/{name}']


def job(name, kind, point, impl, archive):
    key = archive_key(kind, point, impl)
    if kind == 'encode':
        argv = encode_argv(impl, point, name)
    else:
        argv = decode_argv(impl, f'scratch/{name}.{key}.zxc')
    return f'{name}/{kind}/{point}/{impl}', dict(file=name, kind=kind, point=point, impl=impl,
                                                  archive=key, archive_sha256=archive['sha256'], argv=argv)


def validate(raw):
    check(raw['schema'] == 1 and raw['warmups'] == 1 and raw['samples'] == SAMPLES and raw['seed'] == SEED,
          'unknown evidence protocol')
    levels = raw['levels']
    check(levels_arg(','.join(map(str, levels))) == levels, 'invalid levels')
    names = raw['selected_files']
    check(raw['scope'] in ('full', 'quick') and names == list(FILES if raw['scope'] == 'full' else QUICK)
          and set(raw['files']) == set(names), 'wrong file scope')
    bins = set(raw['binaries'])
    check(bins in ({'c', 'current'}, {'c', 'current', 'before'}), 'wrong binary set')
    expected = {}
    for name in names:
        archives = raw['files'][name]['archives']
        check(set(archives) == {archive_key('encode', p, i) for p, i in rows(raw, 'encode')},
              f'incomplete archive matrix: {name}')
        for key, arc in archives.items():
            verifies = arc['verification']
            archive_path = f'scratch/{name}.{key}.zxc'
            check(set(verifies) == bins and all(v['verified_exact'] is True and v['argv'] == decode_argv(i, archive_path)
                                               for i, v in verifies.items()), f'invalid verification: {name}/{key}')
            if key.startswith('c.'):
                check(arc['sha256'] == arc['frozen_sha256'], f'fixed archive mismatch: {name}/{key}')
        for kind in ('encode', 'decode'):
            for p, i in rows(raw, kind):
                jid, spec = job(name, kind, p, i, archives[archive_key(kind, p, i)])
                expected[jid] = spec
    check(expected == raw['jobs'], 'wrong jobs, argv or archive identities')
    # Pass 0 is warmup. Check order AND set coverage so a duplicate cannot
    # stand in for a missing observation, even if counts match.
    count = len(expected)
    obs = raw['observations']
    check(len(obs) == (SAMPLES + 1) * count, 'wrong observation count')
    times = {}
    for p in range(SAMPLES + 1):
        block = obs[p*count:(p+1)*count]
        check({o['job'] for o in block} == expected.keys(), f'duplicate or missing job in pass {p}')
        for pos, o in enumerate(block):
            check(o['pass'] == p and o['order'] == pos and o['phase'] == ('sample' if p else 'warmup')
                  and type(o['elapsed_ns']) is int and o['elapsed_ns'] > 0, f'invalid observation: {o}')
            if p:
                times.setdefault(o['job'], []).append(o['elapsed_ns'])
    return times


def measure(raw, times, names, kind, point, impl):
    key = archive_key(kind, point, impl)
    plain = sum(raw['files'][n]['bytes'] for n in names)
    size = sum(raw['files'][n]['archives'][key]['bytes'] for n in names)
    totals = [sum(ts) for ts in zip(*(times[f'{n}/{kind}/{point}/{impl}'] for n in names), strict=True)]
    return plain, size, median(totals), min(totals), max(totals)


def delta(candidate, ref):
    return 100 * (candidate / ref - 1)


def table(raw, times, scopes, kind):
    enc = kind == 'encode'
    paired = 'before' in raw['binaries']
    headers = ['Scope', 'Point (reference)' if enc else 'Fixed archive', 'Encoder' if enc else 'Decoder',
               'Plain B', 'Archive B']
    if enc:
        headers += ['Archive/plain %', 'Size Δ vs C %']
    headers += ['Median ms', 'Min–max ms', 'MB/s', 'Speed × vs C']
    if paired:
        headers += ['Current/before ×'] + (['Size Δ vs before %'] if enc else [])
    data = [headers, ['---'] * len(headers)]
    for scope, names in scopes:
        for point, impl in rows(raw, kind):
            plain, size, ns, lo, hi = measure(raw, times, names, kind, point, impl)
            ref = MODES.get(point, point)
            has_ref = int(ref[1:]) in raw['levels']
            label = f'{point} ({ref})' if point in MODES and has_ref else point
            row = [scope, label, impl, f'{plain:,}', f'{size:,}']
            if has_ref:
                _, csize, cns, _, _ = measure(raw, times, names, kind, ref, 'c')
            if enc:
                row += [f'{100*size/plain:.3f}', f'{delta(size, csize):+.3f}' if has_ref else '—']
            row += [f'{ns/1e6:.2f}', f'{lo/1e6:.2f}–{hi/1e6:.2f}', f'{plain*1000/ns:.2f}',
                    f'{cns/ns:.3f}' if has_ref else '—']
            if paired:
                if impl == 'current':
                    _, oldsize, oldns, _, _ = measure(raw, times, names, kind, point, 'before')
                    row += [f'{oldns/ns:.3f}'] + ([f'{delta(size, oldsize):+.3f}'] if enc else [])
                else:
                    row += ['—'] * (2 if enc else 1)
            data.append(row)
    return '\n'.join('| ' + ' | '.join(row) + ' |' for row in data)


def render(raw):
    times = validate(raw)
    files = raw['selected_files']
    arcs = sum(len(f['archives']) for f in raw['files'].values())
    sections = [f"# Silesia comparison: {raw['scope']} ({len(files)} files; C levels {','.join(map(str, raw['levels']))})",
                f"Verified: {arcs} archives, {arcs*len(raw['binaries'])} exact cross-decodes; "
                f"{len(raw['jobs'])} CLI jobs/pass, one warmup + seven samples (seed 42)."]
    for kind, title in [('encode', 'Encode + compression'), ('decode', 'Fixed-archive decode')]:
        sections.extend([f'## {title} — aggregate', table(raw, times, [(raw['scope'], files)], kind)])
    summary = '\n\n'.join(sections)
    sections.extend([f"Host: {raw['host']['cpu']}; affinity {raw['host']['affinity']}. C native Release, one worker plus streaming driver; "
                     'Rust portable release, default features, serial. Checksums on, default 512 KiB blocks. '
                     'Load, clock frequency and file-cache residency uncontrolled.',
                     'Median of per-pass elapsed totals; observed range, not confidence bounds. '
                     'Decimal MB/s uses plaintext in both directions. Speed >1 is faster; negative size Δ is smaller. '
                     'Split/C3 and Dense/C7 are reference points, not algorithm equivalents. '
                     'All decoders read identical pinned C bytes within each fixed-archive row group.'])
    for kind, title in [('encode', 'Encode + compression'), ('decode', 'Fixed-archive decode')]:
        sections.extend([f'## {title} — by file', table(raw, times, [(n, [n]) for n in files], kind)])
    return '\n\n'.join(sections) + '\n', summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Validate and re-render raw comparison evidence to stdout.')
    parser.add_argument('raw', type=Path)
    report, _ = render(json.loads(parser.parse_args().raw.read_text()))
    print(report, end='')
