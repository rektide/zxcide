"""Build and snapshot the two native CLIs used in a comparison run (Linux)."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess as sp
import tempfile

from scorecard import sha

ROOT = Path(__file__).resolve().parents[1]
PERF_REV = '5719a96f778226868b29dba621a8c4e302b74d7c'
REPO = 'https://github.com/hellobertrand/zxc'


def command(log, *args, capture=False, env=None):
    argv = list(map(str, args))
    print('$', shlex.join(argv), file=log, flush=True)
    result = sp.run(argv, cwd=ROOT, stdout=sp.PIPE if capture else log,
                    stderr=log, check=True, text=True, env=env)
    return result.stdout.rstrip() if capture else None


def host():
    fields = dict(line.split(':', 1) for line in Path('/proc/cpuinfo').read_text().split('\n\n')[0].splitlines() if ':' in line)
    fields = {k.strip(): v.strip() for k, v in fields.items()}
    affinity = sorted(os.sched_getaffinity(0))
    return dict(platform=platform.platform(), hostname=platform.node(), arch=platform.machine(),
                cpu=fields.get('model name', fields.get('CPU part')), flags=fields.get('flags', fields.get('Features')),
                affinity=affinity, affinity_count=len(affinity), cpu_quota=cpu_quota(), loadavg_start=os.getloadavg())


def cpu_quota(proc=Path('/proc/self')):
    """Smallest finite CPU bandwidth limit among visible cgroup ancestors (v1/v2)."""
    def mount_path(value):
        return Path(re.sub(r'\\([0-7]{3})', lambda m: chr(int(m[1], 8)), value))

    try:
        memberships = [line.split(':', 2) for line in (proc / 'cgroup').read_text().splitlines()]
        mounts = (proc / 'mountinfo').read_text().splitlines()
    except OSError:
        return None
    groups = {kind: Path(path) for _, controllers, path in memberships
              for kind in (['cgroup2'] if not controllers else ['cgroup'] if 'cpu' in controllers.split(',') else [])}
    quotas = []
    for mount in mounts:
        pre, post = mount.split(' - ', 1)
        kind, _, options = post.split()
        if kind not in groups or (kind == 'cgroup' and 'cpu' not in options.split(',')):
            continue
        root, base = map(mount_path, pre.split()[3:5])
        group = groups[kind]
        if '..' in group.parts or not group.is_relative_to(root):
            continue
        leaf = base / group.relative_to(root)
        for path in (leaf, *leaf.parents):
            try:
                if kind == 'cgroup2':
                    quota, period = (path / 'cpu.max').read_text().split()
                    quota = -1 if quota == 'max' else int(quota)
                else:
                    quota = int((path / 'cpu.cfs_quota_us').read_text())
                    period = (path / 'cpu.cfs_period_us').read_text()
                if quota > 0 and int(period) > 0:
                    quotas.append(quota / int(period))
            except (OSError, ValueError):
                pass  # unavailable or delegated controller
            if path == base:
                break
    return min(quotas, default=None)


def snapshot(source, run, label, build):
    dest = Path('bin') / label
    shutil.copy2(source, run / dest)
    return dict(path=str(dest), sha256=sha(run / dest), build=build)


def build_current(run, log):
    # Explicit empty rustflags overrides Cargo config/env target-cpu tuning.
    env = {**os.environ, 'CARGO_ENCODED_RUSTFLAGS': ''}
    argv = ['cargo', 'build', '--locked', '--release', '-p', 'zxc-cli', '--message-format=json-render-diagnostics']
    messages = command(log, *argv, capture=True, env=env)
    artifacts = [obj for obj in map(json.loads, messages.splitlines()) if obj['reason'] == 'compiler-artifact']
    cli, = [a for a in artifacts if a['target']['name'] == 'zxc' and a['executable']]
    status = command(log, 'git', 'status', '--porcelain=v1', '--untracked-files=normal', capture=True)
    build = dict(revision=command(log, 'git', 'rev-parse', 'HEAD', capture=True), dirty=bool(status),
                 status=status, command=argv, rustc=command(log, 'rustc', '-vV', capture=True),
                 cargo=command(log, 'cargo', '-V', capture=True), features=cli['features'],
                 profile=cli['profile'], rustflags=[], lock_sha256=sha(ROOT / 'Cargo.lock'))
    return snapshot(cli['executable'], run, 'current', build)


def build_reference(cache, run, machine, log):
    root = cache / PERF_REV
    root.mkdir(parents=True, exist_ok=True)
    source = root / 'source'
    cached = source.exists()
    if not cached:
        with tempfile.TemporaryDirectory(dir=root) as tmp:
            staged = Path(tmp) / 'source'
            command(log, 'git', 'init', '-q', staged)
            command(log, 'git', '-C', staged, 'fetch', '--depth=1', REPO, PERF_REV)
            command(log, 'git', '-C', staged, 'checkout', '-q', '--detach', 'FETCH_HEAD')
            staged.rename(source)
    rev = command(log, 'git', '-C', source, 'rev-parse', 'HEAD', capture=True)
    status = command(log, 'git', '-C', source, 'status', '--porcelain', '--ignored', capture=True)
    if rev != PERF_REV or status:
        raise ValueError(f'unexpected upstream revision or dirty source: {source}: {rev} {status}')
    cc = shutil.which(os.environ.get('CC', 'cc'))
    if cc is None:
        raise ValueError('CC must name a native C compiler executable')
    # Select the single-config build so the product path is unambiguous.
    options = ['-G', 'Unix Makefiles', '-DCMAKE_BUILD_TYPE=Release', '-DZXC_BUILD_TESTS=OFF',
               '-DZXC_NATIVE_ARCH=ON', '-DBUILD_SHARED_LIBS=OFF', '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON',
               f'-DCMAKE_C_COMPILER={cc}', '-DCMAKE_C_FLAGS=', '-DCMAKE_C_FLAGS_RELEASE=-O3 -DNDEBUG']
    env = {k: v for k, v in os.environ.items() if k not in {'CMAKE_TOOLCHAIN_FILE', 'CFLAGS', 'LDFLAGS'}}
    toolchain = dict(cc=cc, compiler=command(log, cc, '--version', capture=True),
                     cmake=command(log, 'cmake', '--version', capture=True), options=options,
                     host={k: machine[k] for k in ('platform', 'arch', 'cpu', 'flags', 'hostname')})
    key = hashlib.sha256(json.dumps(toolchain, sort_keys=True).encode()).hexdigest()[:20]
    build = root / f'build-{key}'
    configure = ['cmake', '-S', str(source), '-B', str(build), *options]
    compile_ = ['cmake', '--build', str(build), '--parallel', '2']
    command(log, *configure, env=env)
    command(log, *compile_, env=env)
    info = dict(revision=rev, url=REPO, cached_source=cached, source=str(source), toolchain=toolchain,
                commands=[configure, compile_], compile_commands=json.loads((build / 'compile_commands.json').read_text()))
    return snapshot(build / 'zxc', run, 'c', info)
