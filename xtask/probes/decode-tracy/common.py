import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess as sp

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SCRATCH = ROOT / '.test-agent/decode-tracy'
PINS = json.loads((HERE / 'pins.json').read_text())


def require(ok, message):
    if not ok:
        raise RuntimeError(message)


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def write_json(path, value):
    with path.open('x') as f:
        json.dump(value, f, indent=2)
        f.write('\n')


def new_run(path):
    os.umask(0o077)
    path = path.resolve()
    require(path.parent == SCRATCH, f'choose a fresh direct child of {SCRATCH}')
    SCRATCH.mkdir(parents=True, exist_ok=True)
    path.mkdir()
    return path


def perf_lock():
    path = ROOT / '.test-agent/zxc-perf.lock'
    f = path.open('a')
    fcntl.flock(f, fcntl.LOCK_EX)
    return f


def text(*argv, **kwargs):
    return sp.check_output(argv, text=True, **kwargs).strip()


def protocol(public):
    version = (public / 'common/TracyVersion.hpp').read_text()
    names = ('Major', 'Minor', 'Patch')
    actual = '.'.join(re.search(rf'\b{name}\s*=\s*(\d+);', version)[1] for name in names)
    wire = (public / 'common/TracyProtocol.hpp').read_text()
    proto = int(re.search(r'\bProtocolVersion\s*=\s*(\d+);', wire)[1])
    require((actual, proto) == (PINS['tracy']['version'], PINS['tracy']['protocol']),
            f'incompatible client headers at {public}: {actual}, protocol {proto}')
    return {'public': str(public), 'version': actual, 'protocol': proto,
            'version_header_sha256': sha(public / 'common/TracyVersion.hpp'),
            'protocol_header_sha256': sha(public / 'common/TracyProtocol.hpp')}
