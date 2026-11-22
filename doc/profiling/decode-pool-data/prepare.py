#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

parser = argparse.ArgumentParser(description='Prepare the verified original C7 bundle and a new single-frame control.')
for name in ('run', 'zip', 'out'):
    parser.add_argument(name, type=lambda p: Path(p).resolve())
parser.add_argument('--frames', type=Path, help='optional cached C7 frame directory; otherwise regenerate')
a = parser.parse_args()
run, zip_path, out = a.run, a.zip, a.out


def identity(path):
    with path.open('rb') as f:
        return {'bytes': path.stat().st_size, 'sha256': hashlib.file_digest(f, 'sha256').hexdigest()}


out.mkdir(parents=True, exist_ok=False)
assert identity(run / 'raw.json')['sha256'] == (run / 'COMPLETE').read_text().strip()
raw = json.loads((run / 'raw.json').read_text())
assert identity(zip_path)['sha256'] == raw['corpus']['zip_sha256']
assert identity(run / 'bin/c')['sha256'] == raw['binaries']['c']['sha256']
shutil.copy2(run / 'bin/c', out / 'c')
members = []
with zipfile.ZipFile(zip_path) as z, (out / 'plain').open('wb') as p, (out / 'concat.zxc').open('wb') as a:
    for name in raw['selected_files']:
        entry = raw['files'][name]
        blob = z.read(name)
        assert len(blob) == entry['bytes'] and hashlib.sha256(blob).hexdigest() == entry['sha256']
        p.write(blob)
        dest = out / f'{name}.zxc'
        if a.frames:
            shutil.copyfile(a.frames / dest.name, dest)
        else:
            with dest.open('wb') as f:
                subprocess.run([str(out / 'c'), '-qzc', '-T1', '-7'], input=blob, stdout=f, check=True)
        arc = identity(dest)
        assert all(arc[k] == entry['archives']['c.C7'][k] for k in arc)
        with dest.open('rb') as f:
            shutil.copyfileobj(f, a)
        members.append({'file': dest.name, 'plain_bytes': entry['bytes'], 'plain_sha256': entry['sha256'], **arc})
del blob
concat = identity(out / 'concat.zxc')
assert all(concat[k] == raw['bundles']['C7'][k] for k in concat)
plain = identity(out / 'plain')
cmd = [str(out / 'c'), '-qzc', '-T1', '-7', str(out / 'plain')]
with (out / 'single.zxc').open('wb') as f:
    subprocess.run(cmd, stdout=f, check=True)
single = identity(out / 'single.zxc')
# Require a single prologue, checked data blocks, and the ordinary EOF/footer;
# both implementations will also cross-decode through EOF before timing.
with (out / 'single.zxc').open('rb') as f:
    head = f.read(16)
    with (out / 'concat.zxc').open('rb') as bundle:
        assert head == bundle.read(16)
    blocks = 0
    while True:
        header = f.read(8)
        assert len(header) == 8
        if header[0] == 255:
            break
        assert header[0] in (0, 1, 2)
        f.seek(int.from_bytes(header[3:7], 'little') + 4, 1)
        blocks += 1
    assert len(f.read()) == 17 and blocks > 1
(out / 'manifest.json').write_text(json.dumps({'c': raw['binaries']['c'], 'members': members, 'plain': plain,
                                              'concat': concat, 'single': single, 'single_blocks': blocks,
                                              'single_encode_argv': cmd}, indent=2) + '\n')
print(f'{blocks} checked single-frame blocks, {plain["bytes"]:,} plaintext bytes: {out}')
