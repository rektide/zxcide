#!/usr/bin/env python3
"""Pinned W1 AB/BA wall and wait4 child CPU, with interleaved A/A and C."""
import argparse, fcntl, gzip, hashlib, json, os, random, statistics as st, subprocess as sp, time
from prepare import ROOT, S, PIN, CPIN
from race import fmt, identity, snapshot, MANIFEST

def once(cmd, sink):
    start=time.perf_counter_ns()
    with sp.Popen(cmd,stdout=sink,stderr=sp.PIPE) as p:
        err=p.stderr.read(); _,status,u=os.wait4(p.pid,0); p.returncode=os.waitstatus_to_exitcode(status)
    wall=time.perf_counter_ns()-start
    assert p.returncode==0 and not err, (cmd,p.returncode,err)
    return {'wall_ns':wall,'cpu_s':u.ru_utime+u.ru_stime}

def verify(cmd, expected):
    with sp.Popen(cmd,stdout=sp.PIPE,stderr=sp.PIPE) as p:
        h=hashlib.sha256(); n=0
        while b:=p.stdout.read(1<<20): h.update(b); n+=len(b)
        err=p.stderr.read()
        assert p.wait()==0 and not err, (cmd,err)
    out={'bytes':n,'sha256':h.hexdigest()}; assert out==expected,(cmd,out)
    return out

def main():
    p=argparse.ArgumentParser(); p.add_argument('winner'); p.add_argument('--seeds',type=int,nargs='+',default=[621701,621719]); p.add_argument('--only',nargs='+'); p.add_argument('--prefix',default='w1'); a=p.parse_args()
    d=S/a.prefix; d.mkdir(exist_ok=True)
    fix=ROOT/'.test-agent/decode-pool/fixtures'; small=ROOT/'.test-agent/pivco-queue/fixtures'
    man=json.loads(MANIFEST.read_text())
    cases={m['file'][:-4]:(fix/m['file'],{k:m[k] for k in ('bytes','sha256')},{k:m['plain_'+k] for k in ('bytes','sha256')})
           for m in man['members'] if m['file'] in ('mozilla.zxc','sao.zxc')}
    cases['single']=(fix/'single.zxc',man['single'],man['plain'])
    recorded=dict(line.split()[::-1] for line in (ROOT/'doc/profiling/pivco-queue-data/sha256.txt').read_text().splitlines() if line.strip())
    for short,name in (('dense4k','mixed'),('raw4k','noise')):
        arc=small/(name+'12.zxc'); arc_id=identity(arc); plain=identity(small/name)
        assert arc_id['sha256']==recorded['fixtures/'+arc.name] and plain['sha256']==recorded['fixtures/'+name]
        cases[short]=(arc,arc_id,plain)
    bins={'a':S/'cli-a','b':S/f'cli-{a.winner}','c':fix/'c','s':S/f'cli-{a.winner}-scalar'}
    assert identity(bins['c'])['sha256']==man['c']['sha256']
    def command(k,arc):
        flags=['-qdc','-T1'] if k=='c' else ['decode','--threads','1']
        return ['taskset','-c','3',str(bins[k]),*flags,str(arc)]
    names=a.only or ['single','mozilla','sao','dense4k','raw4k']
    comparisons=[(x,i,j) for x in names for i,j in [('a','b'),('a','a')]+([('a','c')] if x in ('single','mozilla','sao') else [])]
    data={'pin':PIN,'c_pin':CPIN,'winner':a.winner,'engines':{k:{'path':str(b),**identity(b)} for k,b in bins.items()},
          'fixtures':{k:[str(t[0]),*t[1:]] for k,t in cases.items()},'comparisons':comparisons,'verification':{},'series':[]}
    with (ROOT/'.test-agent/zxc-perf.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        data['before']=snapshot(3)
        for name in names:
            arc, arc_id, plain=cases[name]; assert identity(arc)==arc_id
            for k in ['a','b','s']+(['c'] if name in ('single','mozilla','sao') else []):
                cmd=command(k,arc); data['verification'][f'{name}/{k}']={'argv':cmd,**verify(cmd,plain)}
        src=bytearray(cases['sao'][0].read_bytes()); src[24+int.from_bytes(src[19:23],'little')] ^= 1
        bad=d/'sao-bad-checksum.zxc'; bad.write_bytes(src)
        for k in bins:
            cmd=command(k,bad); q=sp.run(cmd,stdout=sp.DEVNULL,stderr=sp.PIPE)
            assert q.returncode!=0 and (k=='c' or b'checksum' in q.stderr.lower())
            data['verification'][f'checksum/{k}']={'argv':cmd,'status':q.returncode,'stderr':q.stderr.decode()}
        for seed in a.seeds:
            rng=random.Random(seed); orders=[rng.sample([0,1]*12,24) for _ in comparisons]
            series={'seed':seed,'before':snapshot(3),'pairs':[]}
            with open('/dev/null','wb') as sink:
                for n in range(-1,24):
                    for i in rng.sample(range(len(comparisons)),len(comparisons)):
                        name,x,y=comparisons[i]; first=rng.randrange(2) if n<0 else orders[i][n]; pair=[None,None]
                        for pos in (first,1-first): pair[pos]=once(command((x,y)[pos],cases[name][0]),sink)
                        series['pairs'].append({'round':n,'comparison':i,'first':first,'samples':pair})
            series['after']=snapshot(3); data['series'].append(series)
            with gzip.open(d/'raw.json.gz','wt') as f: json.dump(data,f)
            print(seed,series['before']['load'],series['after']['load'],flush=True)
    with (d/'summary.tsv').open('w') as f:
        print('case\tcomp\tseed\tMB/s a\tMB/s b\tthroughput % [CI]\tCPU s/GB a\tCPU s/GB b\tCPU % [CI]',file=f)
        for i,(name,x,y) in enumerate(comparisons):
            n=cases[name][2]['bytes']
            for s in data['series']:
                pairs=[r['samples'] for r in s['pairs'] if r['comparison']==i and r['round']>=0]
                v=lambda k,e:st.median(p[k][e] for p in pairs)
                print(name,x+'/'+y,s['seed'],*(f'{1000*n/v(k,"wall_ns"):.1f}' for k in (0,1)),
                      fmt([100*(x['wall_ns']/y['wall_ns']-1) for x,y in pairs]),
                      *(f'{1e9*v(k,"cpu_s")/n:.3f}' for k in (0,1)),fmt([100*(y['cpu_s']/x['cpu_s']-1) for x,y in pairs]),sep='\t',file=f)

if __name__=='__main__': main()
