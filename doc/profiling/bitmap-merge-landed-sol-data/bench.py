#!/usr/bin/env python3
"""Build clean Rust pins and reuse the paired width-six CLI benchmark."""
import argparse
import os
from pathlib import Path
import subprocess as sp
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('parent', help='Rust commit before merge landing')
p.add_argument('landed', help='Rust commit with the merge')
p.add_argument('scratch', type=lambda s: Path(s).resolve())
a = p.parse_args()
root = Path.cwd()
bench = root / 'doc/profiling/flat-unpack6-landed-terra-data/bench.py'
a.scratch.mkdir(exist_ok=True, parents=True)
tmp = a.scratch / 'tmp'
tmp.mkdir(exist_ok=True)

def revision(value):
    return sp.check_output(['git', 'rev-parse', '--verify', f'{value}^{{commit}}'], text=True).strip()

pins = (revision(a.parent), revision(a.landed))
env = {**os.environ, 'CARGO_ENCODED_RUSTFLAGS': '', 'CARGO_BUILD_JOBS': '4', 'TMPDIR': str(tmp)}
for label, pin in zip('ab', pins):
    src = a.scratch / f'src-{label}'
    src.mkdir()
    with tempfile.TemporaryFile(dir=tmp) as archive:
        sp.run(['git', 'archive', pin], stdout=archive, check=True)
        archive.seek(0)
        sp.run(['tar', '-x', '-C', str(src)], stdin=archive, check=True)
    target = a.scratch / f'target-{label}'
    command = ['cargo', 'build', '--locked', '--release', '-p', 'zxc-cli']
    env['CARGO_TARGET_DIR'] = str(target)
    with (a.scratch / f'build-{label}.log').open('wb') as log:
        sp.run(['rustc', '-Vv'], env=env, stdout=log, stderr=sp.STDOUT, check=True)
        sp.run(command, cwd=src, env=env, stdout=log, stderr=sp.STDOUT, check=True)
        (a.scratch / f'cli-{label}').write_bytes((target/'release/zxc').read_bytes())
        (a.scratch / f'cli-{label}').chmod(0o700)
        if label == 'b':
            sp.run([*command, '--no-default-features'], cwd=src, env=env, stdout=log, stderr=sp.STDOUT, check=True)
            (a.scratch / 'cli-b-scalar').write_bytes((target/'release/zxc').read_bytes())
            (a.scratch / 'cli-b-scalar').chmod(0o700)

text = bench.read_text()
for old, new in (
    ("scratch = root / '.test-agent/flat6-terra'", f'scratch = Path({str(a.scratch)!r})'),
    ("'2fd99c21', 'b':'0d68cb82'", f"{pins[0]!r}, 'b':{pins[1]!r}"),
    ('(610177, 610439)', '(918173, 884923)'),
):
    assert text.count(old) == 1, old
    text = text.replace(old, new)
# The paired driver already runs native tests under the shared lock, verifies
# exact A/B/C outputs and checksum rejection, and retains A/A controls.
exec(compile(text, str(bench), 'exec'), {'__name__': '__main__', '__file__': str(bench)})
