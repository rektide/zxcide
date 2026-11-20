"""Comparison job definitions, evidence validation and offline Markdown rendering."""
import argparse
from dataclasses import dataclass
import json
import math
from pathlib import Path
import re
from statistics import median

from scorecard import FILES

MODES = {'split': 'C3', 'dense': 'C7'}
QUICK = ('dickens', 'sao')
SAMPLES = 7
SEED = 42
MAX_WORKERS = 256  # Rust Config::MAX_WORKERS; pinned C accepts at least this many.
CPU_FIELDS = ('user_us', 'system_us')


def levels_arg(text):
    values = text.split(',')
    if any(s not in tuple('1234567') for s in values) or len(set(values)) != len(values):
        raise argparse.ArgumentTypeError('levels must be distinct integers 1..7 separated by commas')
    return list(map(int, values))


def workers_arg(text):
    values = text.split(',')
    if (any(not re.fullmatch(r'[1-9][0-9]{0,2}', s) or int(s) > MAX_WORKERS for s in values)
            or len(set(values)) != len(values)):
        raise argparse.ArgumentTypeError(f'decode workers must be distinct integers 1..{MAX_WORKERS} separated by commas')
    return sorted(map(int, values))


def files_arg(text):
    values = text.split(',')
    if not set(values) <= set(FILES) or len(set(values)) != len(values):
        raise argparse.ArgumentTypeError(f'files must be distinct names from {",".join(FILES)}')
    return [name for name in FILES if name in values]


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


def decode_argv(impl, path, workers=1):
    args = ['-qdc', f'-T{workers}'] if impl == 'c' else ['decode']
    if impl != 'c' and workers != 1:
        args += ['--threads', str(workers)]
    return [f'bin/{impl}', *args, path]


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


def validate_v1(raw):
    names = raw['selected_files']
    check(raw['scope'] in ('full', 'quick') and names == list(FILES if raw['scope'] == 'full' else QUICK)
          and set(raw['files']) == set(names), 'wrong file scope')
    bins = set(raw['binaries'])
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
    return observations(raw, expected)


def observations(raw, expected):
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
            cpu = raw['schema'] == 2
            if cpu:
                check(all(type(o.get(k)) is int and o[k] >= 0 for k in CPU_FIELDS), f'invalid CPU observation: {o}')
            if p:
                times.setdefault(o['job'], []).append((o['elapsed_ns'], sum(o[k] for k in CPU_FIELDS))
                                                       if cpu else o['elapsed_ns'])
    return times


def valid_digest(value):
    return isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value) is not None


def valid_identity(entry):
    return type(entry.get('bytes')) is int and entry['bytes'] > 0 and valid_digest(entry.get('sha256'))


def scope_label(raw):
    return 'full (12 files)' if raw['scope'] == 'full' else ', '.join(raw['selected_files'])


def worker_policy(raw):
    policy = {'c': 'Explicit -TN; may use an additional driver thread',
              'current': {str(n): dict(backend='serial' if n == 1 else 'crossbeam',
                                       slots=None if n == 1 else min(2*n, MAX_WORKERS))
                          for n in raw['decode_workers']}}
    if 'before' in raw['binaries']:
        policy['before'] = 'Saved CLI defaults (backend and slots not overridden or introspected)'
    return policy


def budget_lines(host, workers):
    count, quota = host['affinity_count'], host['cpu_quota']
    quota_label = 'unknown (no finite limit discovered)' if quota is None else f'{quota:.6g} CPU equivalents'
    lines = [f"CPU budget: affinity {host['affinity']} ({count} eligible logical CPUs); cgroup CPU quota: {quota_label}."]
    reasons = []
    if count < max(workers):
        reasons.append(f'affinity allows only {count} logical CPUs')
    if quota is not None and quota < max(workers):
        reasons.append(f'cgroup quota is {quota_label}')
    if reasons:
        lines.append(f'CONSTRAINED: {max(workers)} requested workers; {"; ".join(reasons)}. '
                     'Change the launcher affinity or --decode-workers as appropriate (quota is a separate limit).')
    return lines


def archive_path(name, key):
    return f'scratch/{name}.{key}.zxc'


def bundle_path(point):
    return f'scratch/bundle.{point}.zxc'


def bundle_members(raw, point):
    return [dict(file=n, **{k: raw['files'][n]['archives'][f'c.{point}'][k] for k in ('bytes', 'sha256')})
            for n in raw['selected_files']]


def matrix_size(files, levels, rust, workers, concat):
    return files * (levels + 2*rust) + (files + int(concat)) * levels * (1 + rust) * workers


def job_id(name, kind, point, impl, workers=1):
    return f'{name}/{kind}/{point}/{impl}' + (f'/{workers}' if kind == 'decode' else '')


def jobs_v2(raw):
    for name in raw['selected_files']:
        archives = raw['files'][name]['archives']
        for point, impl in rows(raw, 'encode'):
            arc = archives[archive_key('encode', point, impl)]
            yield job_id(name, 'encode', point, impl), dict(file=name, kind='encode', point=point, impl=impl,
                layout='separate', workers=1, archive_sha256=arc['sha256'], argv=encode_argv(impl, point, name))
        for point, impl in rows(raw, 'decode'):
            key = archive_key('decode', point, impl)
            for n in raw['decode_workers']:
                yield job_id(name, 'decode', point, impl, n), dict(file=name, kind='decode', point=point, impl=impl,
                    layout='separate', workers=n, archive_sha256=archives[key]['sha256'],
                    argv=decode_argv(impl, archive_path(name, key), n))
    if raw['concat']:
        for point, impl in rows(raw, 'decode'):
            for n in raw['decode_workers']:
                yield job_id('concat', 'decode', point, impl, n), dict(file=None, kind='decode', point=point, impl=impl,
                    layout='concat', workers=n, archive_sha256=raw['bundles'][point]['sha256'],
                    argv=decode_argv(impl, bundle_path(point), n))


def verification_plan(raw):
    """Deduplicate by exact input digest, executable digest and argv; yield plaintext members too."""
    def record(digest, impl, argv, names):
        binary = raw['binaries'][impl]['sha256']
        key = json.dumps([digest, binary, argv], separators=(',', ':'))
        return key, (dict(archive_sha256=digest, binary_sha256=binary, argv=argv, verified_exact=True), names)

    plan = {}
    for name, entry in raw['files'].items():
        for key, arc in entry['archives'].items():
            for impl in raw['binaries']:
                k, v = record(arc['sha256'], impl, decode_argv(impl, archive_path(name, key)), [name])
                plan[k] = v
    for spec in raw['jobs'].values():
        if spec['kind'] == 'decode':
            names = raw['selected_files'] if spec['layout'] == 'concat' else [spec['file']]
            k, v = record(spec['archive_sha256'], spec['impl'], spec['argv'], names)
            plan[k] = v
    return plan


def validate_v2(raw):
    names = raw['selected_files']
    scope = raw['scope']
    check((scope == 'full' and names == list(FILES) or scope == 'quick' and names == list(QUICK)
           or scope == 'subset' and names and names == files_arg(','.join(names)))
          and set(raw['files']) == set(names), 'wrong file scope')
    workers = raw['decode_workers']
    check(workers == workers_arg(','.join(map(str, workers))), 'invalid decode workers')
    check(type(raw['concat']) is bool and (not raw['concat'] or len(names) >= 2), 'invalid concat scope')
    host = raw['host']
    cpus, quota = host['affinity'], host['cpu_quota']
    check(cpus and all(type(i) is int and i >= 0 for i in cpus) and cpus == sorted(set(cpus))
          and type(host['affinity_count']) is int and host['affinity_count'] == len(cpus)
          and (quota is None or type(quota) in (int, float) and math.isfinite(quota) and quota > 0),
          'invalid CPU budget')
    check(raw['decode_policy'] == worker_policy(raw), 'invalid decode policy')
    for impl, binary in raw['binaries'].items():
        check(binary['path'] == f'bin/{impl}' and valid_digest(binary['sha256']), f'invalid binary identity: {impl}')
    keys = {archive_key('encode', p, i) for p, i in rows(raw, 'encode')}
    for name, entry in raw['files'].items():
        check(valid_identity(entry) and set(entry['archives']) == keys, f'incomplete archive matrix: {name}')
        for key, arc in entry['archives'].items():
            check(valid_identity(arc), f'invalid archive identity: {name}/{key}')
            if key.startswith('c.'):
                check(arc['sha256'] == arc['frozen_sha256'], f'fixed archive mismatch: {name}/{key}')
    points = {f'C{n}' for n in raw['levels']} if raw['concat'] else set()
    check(set(raw['bundles']) == points, 'wrong concat levels')
    for point, bundle in raw['bundles'].items():
        members = bundle_members(raw, point)
        check(valid_identity(bundle) and bundle['members'] == members
              and bundle['bytes'] == sum(m['bytes'] for m in members), f'invalid concat members: {point}')
    times = observations(raw, dict(jobs_v2(raw)))
    expected = {k: record for k, (record, _) in verification_plan(raw).items()}
    actual = raw['verification']
    check(expected == actual and all(r['verified_exact'] is True for r in actual.values()),
          'wrong exact decode verification evidence')
    return times


def validate(raw):
    version = raw.get('schema')
    check(type(version) is int and version in (1, 2) and raw['warmups'] == 1
          and raw['samples'] == SAMPLES and raw['seed'] == SEED, 'unknown evidence protocol')
    check(levels_arg(','.join(map(str, raw['levels']))) == raw['levels'], 'invalid levels')
    check(set(raw['binaries']) in ({'c', 'current'}, {'c', 'current', 'before'}), 'wrong binary set')
    return (validate_v1 if version == 1 else validate_v2)(raw)


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


def render_v1(raw, times):
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


@dataclass(frozen=True)
class Metrics:
    plain: int
    archive: int
    ns: int
    lo: int
    hi: int
    cpu_us: int

    def columns(self):
        return [f'{self.ns/1e6:.3f}', f'{self.lo/1e6:.3f}–{self.hi/1e6:.3f}',
                f'{self.plain*1000/self.ns:.3f}', f'{self.cpu_us*1000/self.plain:.3f}']


def metrics(raw, times, names, kind, point, impl, workers=1, concat=False):
    plain = sum(raw['files'][n]['bytes'] for n in names)
    key = archive_key(kind, point, impl)
    size = (raw['bundles'][point]['bytes'] if concat else
            sum(raw['files'][n]['archives'][key]['bytes'] for n in names))
    ids = [job_id(n, kind, point, impl, workers) for n in (['concat'] if concat else names)]
    # Sum matched samples first, then take independent medians of wall and child CPU.
    wall, cpu = zip(*(tuple(map(sum, zip(*block, strict=True)))
                      for block in zip(*(times[jid] for jid in ids), strict=True)), strict=True)
    return Metrics(plain, size, median(wall), min(wall), max(wall), median(cpu))


def ratio(ref, candidate):
    return f'{ref.ns/candidate.ns:.3f}' if ref else '—'


def table_v2(raw, times, scopes, kind):
    enc = kind == 'encode'
    paired = 'before' in raw['binaries']
    headers = ['Scope'] if enc else ['Scope', 'Layout']
    headers += (['Point (reference)', 'Encoder', 'Plain B', 'Archive B', 'Archive/plain %', 'Size Δ vs C %'] if enc else
                ['Level', 'Plain B', 'Fixed archive B', 'Decoder', 'Workers'])
    headers += ['Median ms', 'Min–max ms', 'MB/s', 'CPU s/GB']
    headers += ['Speed × vs C'] if enc else ['× vs own 1 worker', '× vs C at same N']
    if paired:
        headers += ['Current/before ×'] + (['Size Δ vs before %'] if enc else [])
    data = [headers, ['---'] * len(headers)]
    for scope, names, concat in scopes:
        layout = '1 concatenated launch' if concat else f'{len(names)} file launch' + ('es' if len(names) != 1 else '')
        workers = [1] if enc else raw['decode_workers']
        values = {(p, i, n): metrics(raw, times, names, kind, p, i, n, concat)
                  for p, i in rows(raw, kind) for n in workers}
        for (point, impl, n), candidate in values.items():
            ref = MODES.get(point, point)
            c = values.get((ref, 'c', n))
            size = [f'{candidate.plain:,}', f'{candidate.archive:,}']
            if enc:
                label = f'{point} ({ref})' if point in MODES and c else point
                row = [scope, label, impl, *size, f'{100*candidate.archive/candidate.plain:.3f}',
                       f'{delta(candidate.archive, c.archive):+.3f}' if c else '—']
            else:
                row = [scope, layout, point, *size, impl, str(n)]
            row += candidate.columns()
            if not enc:
                row += [ratio(values.get((point, impl, 1)), candidate)]
            row += [ratio(c, candidate)]
            if paired:
                before = values.get((point, 'before', n)) if impl == 'current' else None
                row += [ratio(before, candidate)]
                if enc:
                    row += [f'{delta(candidate.archive, before.archive):+.3f}' if before else '—']
            data.append(row)
    return '\n'.join('| ' + ' | '.join(row) + ' |' for row in data)


def matrix_lines(raw):
    files, levels, workers = raw['selected_files'], raw['levels'], raw['decode_workers']
    count = matrix_size(len(files), len(levels), len(raw['binaries']) - 1, len(workers), raw['concat'])
    layout = 'separate + concat' if raw['concat'] else 'separate'
    return [f'Silesia comparison: {scope_label(raw)}; C levels {",".join(map(str, levels))}; '
            f'decode workers {",".join(map(str, workers))}; {layout}.',
            *budget_lines(raw['host'], workers),
            f'Matrix: {count:,} jobs/pass × {SAMPLES + 1} passes = {(SAMPLES + 1)*count:,} launches, plus verification.']


def highlights(raw, times):
    files = raw['selected_files']
    lines = []
    for mode, ref in MODES.items():
        if int(ref[1:]) in raw['levels']:
            current = metrics(raw, times, files, 'encode', mode, 'current')
            c = metrics(raw, times, files, 'encode', ref, 'c')
            lines.append(f'{mode.title()}/{ref} encode: {delta(current.archive, c.archive):+.3f}% archive size; '
                         f'{ratio(c, current)}× speed.')
    workers = raw['decode_workers']
    point = f'C{max(raw["levels"])}'
    for n in sorted({min(workers), max(workers)}):
        layouts = []
        for concat in ([False, True] if raw['concat'] else [False]):
            values = {i: metrics(raw, times, files, 'decode', point, i, n, concat) for i in raw['binaries']}
            current, c = values['current'], values['c']
            speed = ratio(c, current)
            comparison = f', {ratio(values["before"], current)}× vs before' if 'before' in values else ''
            layouts.append(f'{"concat" if concat else "separate"}: Rust {current.plain*1000/current.ns:.3f} MB/s, '
                           f'{current.cpu_us*1000/current.plain:.3f} CPU s/GB ({speed}× vs C{comparison})')
        lines.append(f'{point} decode / {n} workers — ' + '; '.join(layouts) + '.')
    return '\n'.join(lines)


def render_v2(raw, times):
    files = raw['selected_files']
    summary = matrix_lines(raw)
    agg = (scope_label(raw), files, False)
    decode = [agg] + ([(scope_label(raw), files, True)] if raw['concat'] else [])
    sections = ['# ' + summary[0], *summary[1:],
                f"Verified: {len(raw['verification'])} distinct exact cross-decodes through EOF; "
                f'one shuffled warmup + seven measured passes (seed {SEED}).']
    titles = [('encode', 'Encode + compression', [agg]), ('decode', 'Fixed-archive decode', decode)]
    sections += [part for kind, title, scopes in titles
                 for part in (f'## {title} — aggregate', table_v2(raw, times, scopes, kind))]
    sections += [f"Host: {raw['host']['cpu']}. Eligible logical CPUs can include SMT siblings, not dedicated cores. "
                 'Load, clock frequency and file-cache residency are uncontrolled.',
                 'Encode: C native Release `-T1`; Rust portable release, default features, serial. '
                 'Checksums on, default 512 KiB blocks. Split/C3 and Dense/C7 are reference points, not algorithm equivalents. '
                 'Negative size Δ is smaller; speed ratios greater than one are faster.',
                 f"Decode: {raw['decode_policy']['c']}. Rust current uses " + ', '.join(
                     f'{n}: {p["backend"]}' + (f', {p["slots"]} slots' if p['slots'] is not None else '')
                     for n, p in raw['decode_policy']['current'].items()) + '.' +
                 (f" Before: {raw['decode_policy']['before']}." if 'before' in raw['binaries'] else ''),
                 'Wall and child user+system CPU totals are summed within each pass, then medianed independently. '
                 'Min–max is observed wall spread, not confidence bounds. MB/s and CPU s/GB use decimal plaintext bytes. '
                 'CPU includes all CLI threads, excludes the Python runner, and can exceed wall time. '
                 'All decoders read identical pinned C bytes within each fixed-archive row group.']
    if raw['concat']:
        sections += ['Concat appends the same fixed frames in manifest order. One concatenated launch and separate-file '
                     'launches cover the same bytes. Their difference includes process/pool startup, input opens, cache state, '
                     'frame boundaries and scheduling. Shuffling jobs does not shuffle frame order or attribute waiting; '
                     'instrumentation is needed to isolate startup or locate ordering bottlenecks.']
    for kind, title, _ in titles:
        scopes = [(n, [n], False) for n in files]
        sections += [f'## {title} — by file', table_v2(raw, times, scopes, kind)]
    return '\n\n'.join(sections) + '\n', highlights(raw, times)


def render(raw):
    times = validate(raw)
    return (render_v1 if raw['schema'] == 1 else render_v2)(raw, times)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Validate and re-render raw comparison evidence to stdout.')
    parser.add_argument('raw', type=Path)
    report, _ = render(json.loads(parser.parse_args().raw.read_text()))
    print(report, end='')
