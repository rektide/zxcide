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
import scorecard


class Protocol(unittest.TestCase):
    def test_levels(self):
        for text in ('', '0', '8', '1,', ',3', '4,4', 'split', '01', '1.5'):
            with self.subTest(text=text), self.assertRaises(argparse.ArgumentTypeError):
                tables.levels_arg(text)
        for text, expected in [('5', [5]), ('1,4,7', [1, 4, 7]), ('7,1', [7, 1])]:
            self.assertEqual(tables.levels_arg(text), expected)

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
        self.args = argparse.Namespace(target=self.root, levels=[1, 4, 7], quick=True, before=None)
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
                raw['observations'].append(dict(job=jid, elapsed_ns=100+p, phase='sample' if p else 'warmup',
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
        self.assertEqual(len(raw['jobs']), 22)  # 2 × (3 C + 2 Rust + 3×2 fixed decode)
        self.assertEqual(len(raw['files']['sao']['archives']), 5)
        self.assertIn((base / 'report.md').read_text(), (base / 'run.log').read_text())
        self.assertEqual(tables.render(raw)[0], (base / 'report.md').read_text())
        self.args.levels = [2]
        self.args.before = base
        paired = runner.publish(self.args)
        r = json.loads((paired / 'raw.json').read_text())
        self.assertEqual(len(r['jobs']), 16)  # 2 × (1 C + 4 Rust + 3 decoders)
        self.assertEqual(len(r['files']['sao']['archives']['c.C2']['verification']), 3)
        self.assertNotIn('split (C3)', (paired / 'report.md').read_text())

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

    def test_decoder_trailing_bytes(self):
        with self.exe.open('a') as out:
            out.write('if [ "$1" = decode ]; then echo extra; fi\n')
        self.fails('decoded bytes mismatch')

    def test_decoder_nonzero_after_correct_output(self):
        with self.exe.open('a') as out:
            out.write('if [ "$1" = decode ]; then exit 17; fi\n')
        self.fails('non-zero exit status 17', runner.sp.CalledProcessError)

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
