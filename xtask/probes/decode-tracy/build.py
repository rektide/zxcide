#!/usr/bin/env python3
"""Replay pinned sources and build matched OFF/ON CLI flavors, with receipts."""
import argparse
import json
from pathlib import Path
import os
import shutil
import subprocess as sp
import tarfile
import tomllib

from common import HERE, ROOT, PINS, new_run, perf_lock, protocol, require, sha, text, write_json


def export_tree(git, revision, dest):
    dest.mkdir()
    with sp.Popen([*git, 'archive', revision], stdout=sp.PIPE) as archive:
        with tarfile.open(fileobj=archive.stdout, mode='r|') as tar:
            tar.extractall(dest, filter='data')
        require(archive.wait() == 0, f'export failed: {revision}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-repo', type=Path)
    p.add_argument('--tracy-repo', type=Path)
    p.add_argument('--tracy-sys-public', type=Path)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--jobs', type=int, default=2)
    p.add_argument('--verify-c', action='store_true')
    p.add_argument('--stock-rust-only', action='store_true')
    a = p.parse_args()
    require(a.jobs > 0, 'jobs must be positive')
    out = new_run(a.out)
    src, tracy = out / 'c-src', out / 'tracy-src'
    cp, tp = PINS['c'], PINS['tracy']
    commands = []
    env = {k: v for k, v in os.environ.items() if k not in {
        'CMAKE_TOOLCHAIN_FILE', 'CFLAGS', 'CXXFLAGS', 'LDFLAGS', 'CARGO_ENCODED_RUSTFLAGS',
        'CARGO_BUILD_TARGET', 'TRACY_CLIENT_LIB', 'TRACY_CLIENT_LIB_PATH', 'TRACY_CLIENT_STATIC',
        'TRACY_CLIENT_SYS_CXXFLAGS', 'DOCS_RS'}}

    def run(label, argv, cwd=ROOT, **updates):
        argv = list(map(str, argv))
        commands.append({'label': label, 'argv': argv, 'cwd': str(cwd), 'env': updates})
        with (out / label).open('xb') as f:
            sp.run(argv, cwd=cwd, env=env | updates, stdout=f, stderr=sp.STDOUT, check=True)

    if a.stock_rust_only:
        revision = PINS['rust']['stock_revision']
        stock_src = out / 'rust-stock-src'
        export_tree(['git', '-C', ROOT], revision, stock_src)
        target = out / 'target'
        with perf_lock():
            run('rust-stock-build.log', ['cargo', 'build', '--locked', '--offline', '--profile', 'profiling',
                                        '-j', a.jobs, '-p', 'zxc-cli'], cwd=stock_src,
                CARGO_TARGET_DIR=str(target), RUSTFLAGS='')
        binary = target / 'profiling/zxc'
        write_json(out / 'stock.json', {'revision': revision, 'path': str(binary), 'sha256': sha(binary),
                   'commands': commands})
        print(out / 'stock.json')
        return

    require(a.base_repo and a.tracy_repo and a.tracy_sys_public, 'supply the three pinned source inputs')

    src.mkdir()
    git_c = ['git', '-C', src]
    run('c-init.log', git_c + ['init', '-q'])
    run('c-fetch.log', git_c + ['fetch', '--depth=1', a.base_repo.resolve().as_uri(), cp['base']])
    require(text(*git_c, 'rev-parse', 'FETCH_HEAD') == cp['base'] and
            text(*git_c, 'rev-parse', 'FETCH_HEAD^{tree}') == cp['base_tree'], 'C base mismatch')
    run('c-checkout.log', git_c + ['switch', '--detach', 'FETCH_HEAD'])
    require(not (src / '.git/objects/info/alternates').exists(), 'borrowed C object store')
    for patch in cp['patches']:
        f = HERE / patch['file']
        require(sha(f) == patch['sha256'], f'patch mismatch: {f}')
        run(f'{f.name}.log', git_c + ['-c', 'core.hooksPath=/dev/null', '-c', 'user.name=Replay',
                                     '-c', 'user.email=replay@localhost', 'am', f])
    require(text(*git_c, 'rev-parse', 'HEAD^{tree}') == cp['tree'] and
            not text(*git_c, 'status', '--porcelain'), 'patched C tree mismatch')
    c_head = text(*git_c, 'rev-parse', 'HEAD')
    git_t = ['git', '-C', a.tracy_repo.resolve()]
    require(text(*git_t, 'rev-parse', f"{tp['revision']}^{{tree}}") == tp['tree'], 'Tracy tree mismatch')
    locked = {x['name']: x for x in tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']}
    for name in ('tracy-client', 'tracy-client-sys'):
        require(all(locked[name][k] == PINS['rust'][name][k] for k in ('version', 'checksum')),
                f'Rust client lock mismatch: {name}')

    def build_c(flavor, *flags):
        dest = out / flavor
        opts = ['-G', 'Unix Makefiles', '-DCMAKE_BUILD_TYPE=Release', '-DZXC_BUILD_TESTS=OFF',
                '-DZXC_NATIVE_ARCH=ON', '-DBUILD_SHARED_LIBS=OFF', '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON',
                f'-DCMAKE_C_COMPILER={shutil.which("cc")}', '-DCMAKE_C_FLAGS=',
                '-DCMAKE_C_FLAGS_RELEASE=-O3 -DNDEBUG', f'-DCMAKE_EXE_LINKER_FLAGS=-Wl,-Map={dest}/link.map']
        run(f'{flavor}-configure.log', ['cmake', '-S', src, '-B', dest, *opts, *flags],
            CXX=str(out / 'no-cxx') if flavor == 'c-off' else shutil.which('c++'))
        run(f'{flavor}-build.log', ['cmake', '--build', dest, '--parallel', a.jobs])
        binary = dest / 'zxc'
        run(f'{flavor}-deps.log', ['ldd', binary])
        run(f'{flavor}-nm.log', ['nm', '-A', dest / 'libzxc.a', binary])
        return binary

    def build_rust(profile, flavor, *flags):
        dest = out / 'rust-target'
        updates = {'CARGO_TARGET_DIR': str(dest), 'RUSTFLAGS': ''}
        run(f'{flavor}-features.log', ['cargo', 'tree', '--locked', '--offline', '-e', 'features',
                                     '-p', 'zxc-cli', *flags], **updates)
        run(f'{flavor}-build.log', ['cargo', 'build', '--locked', '--offline', '-vv', '--profile', profile,
                                   '-j', a.jobs, '-p', 'zxc-cli', *flags], **updates)
        binary = out / flavor
        shutil.copy2(dest / profile / 'zxc', binary)
        run(f'{flavor}-deps.log', ['ldd', binary])
        run(f'{flavor}-nm.log', ['nm', '-A', binary])
        return binary

    rust = {'head': text('git', '-C', ROOT, 'rev-parse', 'HEAD'),
            'dirty_paths': text('git', '-C', ROOT, 'status', '--porcelain').splitlines(),
            'rustc': text('rustc', '-vV'), 'locked_client_public': protocol(a.tracy_sys_public.resolve())}
    require(text('git', '-C', ROOT, 'hash-object', ROOT / 'crates/codec/src/block.rs') ==
            text('git', '-C', ROOT, 'rev-parse', PINS['rust']['spans_revision'] + ':crates/codec/src/block.rs'),
            'Rust span source mismatch')
    with perf_lock():
        binaries = dict(c_off=build_c('c-off', '-DZXC_PROFILE_TRACY=OFF'))
        require(not tracy.exists(), 'OFF build unexpectedly depends on an experiment Tracy export')
        export_tree(git_t, tp['revision'], tracy)
        on = ['-DZXC_PROFILE_TRACY=ON', '-DZXC_INSTALL=OFF', f'-DZXC_TRACY_SOURCE={tracy}']
        binaries['c_cap'] = build_c('c-cap', *on)
        if a.verify_c:
            binaries['c_verify'] = build_c('c-verify', *on, '-DZXC_TRACY_VERIFY=ON')
        binaries['rust_off'] = build_rust('profiling', 'rust-off')
        binaries['rust_cap'] = build_rust('profiling', 'rust-cap', '--features', 'tracy')
        binaries['rust_release'] = build_rust('release', 'rust-release')
    write_json(out / 'build.json', {'pins': PINS, 'c_replay_head': c_head, 'rust': rust,
               'c_client_public': protocol(tracy / 'public'), 'commands': commands,
               'cmake': text('cmake', '--version'), 'cc': text('cc', '--version'),
               'binaries': {k: {'path': str(v), 'sha256': sha(v)} for k, v in binaries.items()},
               'compile_commands': {k: json.loads((v.parent / 'compile_commands.json').read_text())
                                    for k, v in binaries.items() if k.startswith('c_')}})
    print(out / 'build.json')


if __name__ == '__main__':
    main()
