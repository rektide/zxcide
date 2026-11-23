#!/usr/bin/env python3
"""Two randomized paired CLI series. Run from repository root after build.sh."""
import fcntl, gzip, hashlib, json, math, os, platform, random, statistics, subprocess, time
from pathlib import Path

root = Path.cwd()
scratch = root / '.test-agent/flat6-terra'
fixtures = root / '.test-agent/decode-pool/fixtures'
manifest = json.loads((root / 'doc/profiling/decode-pool-data/fixture-manifest.json').read_text())
arms = {'a': scratch/'cli-a', 'b': scratch/'cli-b', 'c': fixtures/'c', 'scalar': scratch/'cli-b-scalar'}
cases = {e['file'][:-4]: e for e in manifest['members'] if e['file'] in ('mozilla.zxc', 'sao.zxc')}
cases['single'] = {**manifest['single'], 'plain_bytes': manifest['plain']['bytes'], 'plain_sha256': manifest['plain']['sha256']}

def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

def command(arm, name, workers, path=None):
    mask = '3' if workers == 1 else '0-7'
    flags = ['-qdc', f'-T{workers}'] if arm == 'c' else ['decode', '--threads', str(workers)]
    return ['taskset', '-c', mask, str(arms[arm]), *flags, str(path or fixtures/(name+'.zxc'))]

def snapshot():
    paths = {'load': '/proc/loadavg', 'governor': '/sys/devices/system/cpu/cpu3/cpufreq/scaling_governor',
             'siblings': '/sys/devices/system/cpu/cpu3/topology/thread_siblings_list', 'boost': '/sys/devices/system/cpu/cpufreq/boost'}
    return {'time': time.time(), **{k: Path(v).read_text().strip() if Path(v).exists() else None for k,v in paths.items()}}

def exact(cmd, meta):
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    h = hashlib.sha256(); size = 0
    while buf := p.stdout.read(1048576):
        h.update(buf); size += len(buf)
    err = p.stderr.read(); rc = p.wait()
    assert (rc, err, size, h.hexdigest()) == (0, b'', meta['plain_bytes'], meta['plain_sha256']), (cmd, rc, err, size, h.hexdigest())

def once(cmd, sink):
    start = time.perf_counter_ns()
    with subprocess.Popen(cmd, stdout=sink, stderr=subprocess.PIPE) as p:
        err = p.stderr.read()
        _, status, usage = os.wait4(p.pid, 0)
        p.returncode = os.waitstatus_to_exitcode(status)
    wall = time.perf_counter_ns()-start
    assert p.returncode == 0 and not err, (cmd, p.returncode, err)
    return {'wall_ns': wall, 'cpu_s': usage.ru_utime + usage.ru_stime}

comparisons = [(name, w, x, y) for name in ('single','mozilla','sao') for w in (1,8)
               for x,y in (('a','b'), ('c','b'), ('a','a'))]
data = {'revision': {'a':'2fd99c21', 'b':'0d68cb82'}, 'binaries': {k: {'path': str(v), 'sha256':digest(v)} for k,v in arms.items()},
        'fixtures': cases, 'comparisons': comparisons, 'platform':platform.platform(), 'verification':[], 'series':[]}
with (root/'.test-agent/zxc-perf.lock').open('a') as lock:
    fcntl.flock(lock, fcntl.LOCK_EX)
    data['before'] = snapshot()
    with (scratch/'tests.log').open('wb') as log:
        env = {**os.environ, 'TMPDIR':str(scratch/'tmp'), 'CARGO_TARGET_DIR':str(scratch/'target-b'), 'CARGO_ENCODED_RUSTFLAGS':'', 'RUSTFLAGS':'', 'CARGO_BUILD_JOBS':'4'}
        subprocess.run(['cargo','nextest','run','--locked','--release','--workspace'], cwd=scratch/'src-b', env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        subprocess.run(['cargo','nextest','run','--locked','--release','-p','zxc-codec','--no-default-features'], cwd=scratch/'src-b', env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    assert digest(arms['c']) == manifest['c']['sha256']
    for name, meta in cases.items():
        path = fixtures/(name+'.zxc')
        assert path.stat().st_size == meta['bytes'] and digest(path) == meta['sha256']
        for w in (1,8):
            for arm in ('a','b','c'):
                exact(command(arm,name,w), meta)
                data['verification'].append([arm,name,w])
        exact(command('scalar',name,1),meta)
        data['verification'].append(['scalar',name,1])
    # The checksum follows the first data-block body (header length is little endian).
    src = bytearray((fixtures/'sao.zxc').read_bytes())
    off = 24 + int.from_bytes(src[19:23], 'little')
    src[off] ^= 1
    bad = scratch/'sao-bad-checksum.zxc'; bad.write_bytes(src)
    for arm in ('a','b','c','scalar'):
        p = subprocess.run(command(arm,'sao',1,bad), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        assert p.returncode != 0 and (arm == 'c' or b'checksum' in p.stderr.lower()), (arm,p.returncode,p.stderr)
        data['verification'].append([arm,'sao-bad-checksum',p.returncode,p.stderr.decode(errors='replace')])
    print('verification complete', flush=True)
    for seed in (610177, 610439):
        rng = random.Random(seed)
        orders = []
        for _ in comparisons:
            o = [0,1]*12; rng.shuffle(o); orders.append(o)
        series = {'seed':seed, 'before':snapshot(), 'pairs':[]}
        with open('/dev/null','wb') as sink:
            for n in range(-1,24):
                indices = list(range(len(comparisons))); rng.shuffle(indices)
                for i in indices:
                    name,w,x,y = comparisons[i]; first = rng.randrange(2) if n < 0 else orders[i][n]
                    sample = [None,None]
                    for pos in (first,1-first):
                        sample[pos] = once(command((x,y)[pos],name,w), sink)
                    series['pairs'].append({'round':n,'comparison':i,'first':first,'samples':sample})
        series['after'] = snapshot(); data['series'].append(series)
        with gzip.open(scratch/'raw.json.gz','wt') as f: json.dump(data,f)
        print('series',seed,'complete',series['before']['load'],series['after']['load'],flush=True)
    data['after'] = snapshot()
    with gzip.open(scratch/'raw.json.gz','wt') as f: json.dump(data,f)

def ci(values):
    s=sorted(values); n=len(s); tail=0
    for k in range(n//2):
        tail+=math.comb(n,k)
        if 40*tail > 2**n:
            rank=max(0,k-1); return s[rank],s[-1-rank]
    return s[0],s[-1]

with (scratch/'summary.tsv').open('w') as f:
    print('case\tworkers\tcomparison\tseries\tMBps x\tMBps y\tthroughput % [CI]\tCPU s/GB x\tCPU s/GB y\tCPU % [CI]',file=f)
    for i,(name,w,x,y) in enumerate(comparisons):
        size=cases[name]['plain_bytes']
        for s in data['series']:
            pairs=[r['samples'] for r in s['pairs'] if r['comparison']==i and r['round']>=0]
            wall=[100*(a['wall_ns']/b['wall_ns']-1) for a,b in pairs]
            cpu=[100*(b['cpu_s']/a['cpu_s']-1) for a,b in pairs]
            med=lambda v:statistics.median(v)
            fmt=lambda v:f'{med(v):+.2f} [{ci(v)[0]:+.2f},{ci(v)[1]:+.2f}]'
            print(name,w,f'{x}/{y}',s['seed'],f'{size*1000/med([p[0]["wall_ns"] for p in pairs]):.1f}',f'{size*1000/med([p[1]["wall_ns"] for p in pairs]):.1f}',fmt(wall),f'{med([p[0]["cpu_s"] for p in pairs])*1e9/size:.3f}',f'{med([p[1]["cpu_s"] for p in pairs])*1e9/size:.3f}',fmt(cpu),sep='\t',file=f)
