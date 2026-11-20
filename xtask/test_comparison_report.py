"""Offline comparison-report publication and aggregation regression tests."""
import argparse
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import comparison_report as runner
import comparison_tables as tables
from comparison_build import cpu_quota
import scorecard


class Protocol(unittest.TestCase):
    def test_levels(self):
        for text in ('', '0', '8', '1,', ',3', '4,4', 'split', '01', '1.5'):
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                tables.levels_arg(text)
        for text, expected in [('5', [5]), ('1,4,7', [1, 4, 7]), ('7,1', [7, 1])]:
            self.assertEqual(tables.levels_arg(text), expected)

    def test_workers_and_files(self):
        for text in ('', '0', '257', '1,', '1,1', '+1', '01', '1.5', '²'):
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                tables.workers_arg(text)
        self.assertEqual(tables.workers_arg('256,8,4'), [4, 8, 256])
        for text in ('', 'sao,', 'sao,sao', 'unknown'):
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                tables.files_arg(text)
        self.assertEqual(tables.files_arg('sao,mozilla'), ['mozilla', 'sao'])
        self.assertEqual(tables.matrix_size(dict(schema=3, selected_files=runner.FILES, levels=[1, 3, 7],
                         binaries={'c': {}, 'current': {}}, decode_workers=[1, 4, 8], concat=False)), 420)

    def test_cpu_budget(self):
        temp_root = runner.ROOT / 'target'
        temp_root.mkdir(exist_ok=True)
        for version in (1, 2):
            with self.subTest(version=version), tempfile.TemporaryDirectory(dir=temp_root) as tmp:
                root = Path(tmp) / 'mount point'
                leaf = root / 'batch' / 'worker'
                leaf.mkdir(parents=True)
                proc = Path(tmp)
                (proc / 'cgroup').write_text('0::/slice/batch/worker\n' if version == 2 else '3:cpu,cpuacct:/slice/batch/worker\n')
                escaped = str(root).replace(' ', r'\040')
                (proc / 'mountinfo').write_text(f'35 26 0:32 /slice {escaped} rw - ' +
                    ('cgroup2 cgroup2 rw\n' if version == 2 else 'cgroup cgroup rw,cpu,cpuacct\n'))
                self.assertIsNone(cpu_quota(proc))
                for path, quota in [(leaf, 800_000), (leaf.parent, 150_000), (root, -1)]:
                    if version == 2:
                        (path / 'cpu.max').write_text(f'{quota if quota > 0 else "max"} 100000\n')
                    else:
                        (path / 'cpu.cfs_quota_us').write_text(str(quota))
                        (path / 'cpu.cfs_period_us').write_text('100000\n')
                self.assertEqual(cpu_quota(proc), 1.5)
        lines = tables.budget_lines(dict(affinity=[12, 13], affinity_count=2, cpu_quota=1.5), [1, 4, 8])
        self.assertIn('CONSTRAINED: 8 requested workers; affinity allows only 2 logical CPUs; cgroup quota is 1.5', lines[1])

    def test_pass_aggregation_and_ratio_directions(self):
        names = ['a', 'b']
        raw = dict(files={n: dict(bytes=100_000, archives={'c.C3': {'bytes': 40_000},
                         'current.split': {'bytes': 30_000}, 'before.split': {'bytes': 60_000},
                         'current.dense': {'bytes': 20_000}, 'before.dense': {'bytes': 20_000}})
                          for n in names}, levels=[3], binaries={'c': {}, 'current': {}, 'before': {}})
        # The sum of file medians is 2 ms, but the median pass total is 10 ms.
        times = {}
        for n, series in zip(names, ([9,9,9,1,1,1,1], [1,1,1,9,9,9,1])):
            for impl, point, factor in [('c','C3',2), ('current','split',1), ('before','split',3),
                                        ('current','dense',1), ('before','dense',3)]:
                times[f'{n}/encode/{point}/{impl}'] = [x*1_000_000*factor for x in series]
        result = tables.table(raw, times, [('all', names)], 'encode')
        self.assertIn('| all | split (C3) | current | 200,000 | 60,000 | 30.000 | -25.000 | 10.00 | '
                      '2.00–10.00 | 20.00 | 2.000 | 3.000 | -50.000 |', result)

    def test_v2_independent_cpu_median_and_decode_references(self):
        names = ['mozilla', 'sao']
        raw = dict(files={n: dict(bytes=100_000, archives={'c.C7': {'bytes': 40_000}}) for n in names},
                   bundles={'C7': {'bytes': 80_000}}, levels=[7], decode_workers=[1, 4],
                   binaries={'c': {}, 'current': {}, 'before': {}})
        times = {}
        # Both metrics need per-pass sums: wall median=10 ms not 2 ms; CPU median=20 ms not 4 ms.
        cases = zip(names, ([9,9,9,1,1,1,1], [1,1,1,9,9,9,1]), ([2,2,2,2,18,18,18], [18,18,18,2,2,2,2]))
        for n, walls, cpus in cases:
            for impl, factor in [('c', 2), ('current', 1), ('before', 3)]:
                for workers in raw['decode_workers']:
                    times[tables.job_id(n, 'decode', 'C7', impl, workers)] = [
                        (w*1_000_000*factor*(4 if workers == 1 else 1), u*1000) for w, u in zip(walls, cpus)]
        for impl in raw['binaries']:
            for workers in raw['decode_workers']:
                times[tables.job_id('concat', 'decode', 'C7', impl, workers)] = [(5_000_000, 30_000)] * 7
        scopes = [('mozilla, sao', names, False), ('mozilla, sao', names, True)]
        result = tables.table_v2(raw, times, scopes, 'decode')
        self.assertIn('| mozilla, sao | 2 file launches | C7 | 200,000 | 80,000 | current | 4 | 10.000 | '
                      '2.000–10.000 | 20.000 | 100.000 | 4.000 | 2.000 | 3.000 |', result)
        self.assertIn('| mozilla, sao | 1 concatenated launch | C7 | 200,000 | 80,000 | current | 4 | '
                      '5.000 | 5.000–5.000 | 40.000 | 150.000 | 1.000 | 1.000 | 1.000 |', result)
        raw['decode_workers'] = [4]
        self.assertIn('20.000 | 100.000 | — | 2.000 | 3.000 |', tables.table_v2(raw, times, scopes, 'decode'))


class Publication(unittest.TestCase):
    """Use a twelve-member tiny ZIP and child executables that copy bytes.

    Exercise real acquisition/hash/pipe/exit/publication paths. Replace only
    builds and the expensive sampled invocations (covered by live acceptance).
    """
    def setUp(self):
        temp_root = runner.ROOT / 'target'
        temp_root.mkdir(exist_ok=True)
        tmp = tempfile.TemporaryDirectory(dir=temp_root)
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.args = argparse.Namespace(target=self.root, levels=[1, 4, 7], quick=True, files=None, before=None,
                                       decode_workers=[1], concat=False)
        self.zip = self.root / 'scorecard/silesia.zip'
        self.zip.parent.mkdir()
        files, hashes = {}, {}
        with zipfile.ZipFile(self.zip, 'w') as z:
            for name in runner.FILES:
                text = name.encode() + b'\x00\xff'
                digest = hashlib.sha256(text).hexdigest()
                z.writestr(name, text)
                files[name] = dict(bytes=len(text), sha256=digest, archives={k: dict(sha256=digest) for k in ('c3','c7')})
                hashes[name] = {str(i): digest for i in range(1, 8)}
        zip_sha = scorecard.sha(self.zip)
        manifest = self.root / 'old.json'
        manifest.write_text(json.dumps({'files': files}))
        self.pins = self.root / 'levels.json'
        self.pins.write_text(json.dumps(dict(files=hashes, upstream_revision=runner.PERF_REV, corpus_zip_sha256=zip_sha)))
        self.exe = self.root / 'copy'
        self.exe.write_text('#!/bin/sh\nfor last do :; done\ncat "$last"\n')
        self.exe.chmod(0o700)
        stack = contextlib.ExitStack()
        self.addCleanup(stack.close)
        for module, key, value in [(runner, 'MANIFEST', manifest), (runner, 'LEVEL_HASHES', self.pins),
                                   (runner, 'ZIP_SHA', zip_sha), (scorecard, 'ZIP_SHA', zip_sha)]:
            stack.enter_context(patch.object(module, key, value))
        stack.enter_context(patch.object(runner, 'build_current', lambda run, log: runner.snapshot(self.exe, run, 'current', {})))
        stack.enter_context(patch.object(runner, 'build_reference', lambda cache, run, host, log: runner.snapshot(self.exe, run, 'c', {})))
        stack.enter_context(patch.object(runner, 'sample', self.sample))
        stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        stack.enter_context(contextlib.redirect_stderr(io.StringIO()))

    def sample(self, raw, run, log):
        for p in range(8):
            for pos, jid in enumerate(raw['jobs']):
                raw['observations'].append(dict(job=jid, elapsed_ns=100+p, user_us=15, system_us=5,
                                                phase='sample' if p else 'warmup',
                                                order=pos, **{'pass':p}))

    def fails(self, message, error=ValueError):
        runs = self.root / 'comparison-report/runs'
        existing = set(runs.iterdir()) if runs.exists() else set()
        with self.assertRaisesRegex(error, message):
            runner.publish(self.args)
        new, = set(runs.iterdir()) - existing
        self.assertTrue(new.name.startswith('incomplete-'))
        self.assertIn(message, (new / 'run.log').read_text())
        for path in ('COMPLETE', 'raw.json', 'report.md', 'scratch'):
            self.assertFalse((new / path).exists())

    def test_nondefault_points_and_singleton_paired(self):
        base = runner.publish(self.args)
        raw = json.loads((base / 'raw.json').read_text())
        self.assertEqual(len(raw['jobs']), 30)  # 2 files × 5 sources × (encode + 2 decoders)
        self.assertEqual(len(raw['files']['sao']['archives']), 5)
        self.assertIn((base / 'report.md').read_text(), (base / 'run.log').read_text())
        self.assertEqual(tables.render(raw)[0], (base / 'report.md').read_text())
        self.args.levels = [2]
        self.args.before = base
        paired = runner.publish(self.args)
        r = json.loads((paired / 'raw.json').read_text())
        self.assertEqual(len(r['jobs']), 40)  # 2 files × 5 sources × (encode + 3 decoders)
        for src in ('current', 'before'):
            for dec in ('current', 'before', 'c'):
                self.assertEqual(r['jobs'][f'sao/decode/{src}.dense/{dec}/1']['argv'][-1],
                                 f'scratch/sao.{src}.dense.zxc')
        self.assertEqual(len(r['verification']), 30)  # 2 files × 5 archives × 3 decoders; serial timed jobs reused
        self.assertNotIn('split (C3)', (paired / 'report.md').read_text())

    def test_v1_baseline_only_imports_binary_and_build(self):
        base = runner.publish(self.args)
        path = base / 'raw.json'
        raw = json.loads(path.read_text())
        raw['schema'] = 1
        raw['jobs'] = {}
        for n, data in raw['files'].items():
            arcs = data['archives']
            for key, arc in arcs.items():
                arc['verification'] = {i: dict(verified_exact=True, argv=tables.decode_argv(i, tables.archive_path(n, key)))
                                       for i in raw['binaries']}
            for kind in ('encode', 'decode'):
                for p, i in tables.rows(raw, kind):
                    jid, spec = tables.job(n, kind, p, i, arcs[tables.archive_key(kind, p, i)])
                    raw['jobs'][jid] = spec
        raw['observations'].clear()
        self.sample(raw, base, None)
        for o in raw['observations']:
            del o['user_us'], o['system_us']
        self.assertNotIn('CPU s/GB', tables.render(raw)[0])
        path.write_text(json.dumps(raw))
        (base / 'COMPLETE').write_text(scorecard.sha(path))
        self.args.before = base
        self.args.levels = [2]
        self.args.decode_workers = [4]
        paired = runner.publish(self.args)
        current = json.loads((paired / 'raw.json').read_text())
        self.assertEqual(current['binaries']['before']['sha256'], raw['binaries']['current']['sha256'])
        self.assertEqual(current['binaries']['before']['build'], raw['binaries']['current']['build'])
        self.assertEqual(current['levels'], [2])
        self.assertIn('sao/decode/C2/before/4', current['jobs'])
        (base / 'bin/current').write_bytes(b'wrong old binary')
        self.fails('baseline executable checksum mismatch')

    def test_bad_cached_zip(self):
        self.zip.write_bytes(b'corrupt')
        self.fails('corpus ZIP checksum mismatch')
        self.assertEqual(self.zip.read_bytes(), b'corrupt')

    def test_wrong_fixed_archive(self):
        pins = json.loads(self.pins.read_text())
        pins['files']['sao']['4'] = '0'*64
        self.pins.write_text(json.dumps(pins))
        self.fails('fixed archive hash mismatch')

    def test_wrong_baseline(self):
        base = runner.publish(self.args)
        (base / 'bin/current').write_bytes(b'wrong binary')
        self.args.before = base
        self.fails('baseline executable checksum mismatch')

    def test_misdirected_verifier_or_timed_decoder(self):
        base = runner.publish(self.args)
        for label in ('verified', 'timed'):
            raw = json.loads((base / 'raw.json').read_text())
            if label == 'verified':
                record = next(v for v in raw['verification'].values() if v['argv'] ==
                              ['bin/current', 'decode', 'scratch/sao.c.C4.zxc'])
            else:
                record = raw['jobs']['sao/decode/C4/current/1']
            record['argv'][-1] = 'scratch/sao.current.split.zxc'
            with self.subTest(label=label), self.assertRaises(ValueError):
                tables.render(raw)

    def test_decoder_trailing_bytes(self):
        with self.exe.open('a') as out:
            out.write('if [ "$1" = decode ]; then echo extra; fi\n')
        self.fails('decoded bytes mismatch')

    def test_decoder_nonzero_after_correct_output(self):
        with self.exe.open('a') as out:
            out.write('if [ "$1" = decode ]; then exit 17; fi\n')
        self.fails('non-zero exit status 17')

    def concat(self):
        self.args.quick = False
        self.args.files = tables.files_arg('sao,mozilla')
        self.args.levels = [1, 7]
        self.args.decode_workers = [1, 4, 8]
        self.args.concat = True

    def test_concat_matrix_and_digests(self):
        self.concat()
        base = runner.publish(self.args)
        raw = json.loads((base / 'raw.json').read_text())
        self.assertEqual(len(raw['jobs']), 80)
        self.assertEqual(len(raw['verification']), 72)
        for point, key in [('C1', 'c.C1'), ('C7', 'c.C7'), ('current.split', 'current.split'),
                           ('current.dense', 'current.dense')]:
            bundle = raw['bundles'][point]
            self.assertEqual([m['file'] for m in bundle['members']], ['mozilla', 'sao'])
            self.assertEqual(bundle['sha256'], hashlib.sha256(b'mozilla\x00\xffsao\x00\xff').hexdigest())
            for layout, name, expected in [('concat', None, bundle['sha256']), *[
                     ('separate', f, raw['files'][f]['archives'][key]['sha256']) for f in raw['selected_files']]]:
                jobs = [j for j in raw['jobs'].values() if j['kind'] == 'decode' and j['point'] == point
                        and j['layout'] == layout and j['file'] == name]
                self.assertEqual(len(jobs), 6)
                self.assertEqual({j['archive_sha256'] for j in jobs}, {expected})
                path = f'scratch/bundle.{point}.zxc' if name is None else f'scratch/{name}.{key}.zxc'
                self.assertEqual({j['argv'][-1] for j in jobs}, {path})

        faults = [('worker job', lambda r: r['jobs'].pop('sao/decode/C7/current/8'), 'wrong jobs'),
                  ('worker exact check', lambda r: r['verification'].pop(next(k for k in r['verification'] if
                      '"--threads","8"' in k)), 'wrong exact decode'),
                  ('CPU sample', lambda r: r['observations'][-1].pop('user_us'), 'invalid CPU')]
        for label, corrupt, message in faults:
            bad = json.loads((base / 'raw.json').read_text())
            corrupt(bad)
            with self.subTest(label=label), self.assertRaisesRegex(ValueError, message):
                tables.render(bad)

    def test_decoder_truncates_at_second_frame(self):
        self.concat()
        self.exe.write_text('#!/bin/sh\nfor last do :; done\n'
                            'case "$last" in *bundle*) head -c 9 "$last";; *) cat "$last";; esac\n')
        self.fails('decoded bytes mismatch at sao: bin/c -qdc -T1 scratch/bundle.C1.zxc')

    def test_worker_flags_rejected(self):
        self.args.decode_workers = [4]
        self.exe.write_text('#!/bin/sh\nif [ "$2" = --threads ]; then exit 17; fi\nfor last do :; done\ncat "$last"\n')
        self.fails('try --decode-workers 1 or omit --concat')

    def test_missing_and_duplicate_samples(self):
        for fault, message in [('missing', 'wrong observation count'), ('duplicate', 'duplicate or missing job')]:
            def broken(raw, run, log):
                self.sample(raw, run, log)
                obs = raw['observations']
                if fault == 'missing':
                    obs.pop()
                else:
                    obs[-1]['job'] = obs[-2]['job']
            with self.subTest(fault=fault), patch.object(runner, 'sample', broken):
                self.fails(message)


if __name__ == '__main__':
    unittest.main()
