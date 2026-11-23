#!/usr/bin/env python3
"""Use the existing locked resident grouped-PMU protocol, with interleaved A/A."""
import argparse, fcntl, gzip, json, math, os, random, select, statistics as st, subprocess as sp, sys
from contextlib import ExitStack
from pathlib import Path
from tempfile import TemporaryDirectory
from prepare import ROOT, S, PIN, CPIN
sys.path.insert(0,str(ROOT/'doc/profiling/decode-replay-data'))
from perf import MANIFEST, REPEATS, capture, identity, measure, request, snapshot

def ci(seq):
    s=sorted(seq); n=len(s); tail=0
    for k in range(n//2):
        tail+=math.comb(n,k)
        if 40*tail>2**n:
            rank=max(0,k-1); return s[rank],s[-rank-1]
    return s[0],s[-1]

def fmt(seq):
    lo,hi=ci(seq)
    return f'{st.median(seq):+.2f} [{lo:+.2f},{hi:+.2f}]'

def report(meta,path):
    with path.open('w') as f:
        print('series\tcase\tcomparison\tn\tcycles/B a\tcycles/B b\tcycles % [CI]\tinsns/B a\tinsns/B b\tinsns % [CI]\tCPU % [CI]',file=f)
        for case,d in meta['cases'].items():
            for comp in d['comparisons']:
                pairs=[r['samples'] for r in d['pairs'] if r['comparison']==comp]
                denom=d['units']*d['repeats']
                val=lambda pos,e: f'{st.median(p[pos][e] for p in pairs)/denom:.3f}'
                pct=lambda e: fmt([100*(y[e]/x[e]-1) for x,y in pairs])
                print(meta['seed'],case,'/'.join(comp),len(pairs),val(0,'cycles:u'),val(1,'cycles:u'),pct('cycles:u'),
                     val(0,'instructions:u'),val(1,'instructions:u'),pct('instructions:u'),pct('thread_cpu_ns'),sep='\t',file=f)

def run_case(case,conf,comparisons,argv,cpu,rng,pairs,out):
    labels=list(argv)
    ready={}; runs=[]; verify={}
    with TemporaryDirectory(dir=S/'tmp') as t:
        for k in labels:
            if k=='aa': continue
            dest=Path(t)/k
            with dest.open('wb') as fp:
                sp.run([x if x!='serve' else 'verify' for x in argv[k]],stdout=fp,check=True)
            verify[k] = identity(dest)
            assert verify[k]['bytes']==conf['units']
            assert verify[k]==conf.get('expected',verify['a']), (case,k,verify)
    with ExitStack() as stack:
        workers={}
        for k in labels:
            w=stack.enter_context(sp.Popen(argv[k],stdin=sp.PIPE,stdout=sp.PIPE))
            stack.callback(w.stdin.close)
            assert select.select([w.stdout],[],[],60)[0],argv[k]
            fields=w.stdout.readline().decode().split()
            assert len(fields)==5 and int(fields[1])==conf['count'] and int(fields[2])==conf['units'],fields
            assert os.sched_getaffinity(w.pid)=={cpu} and len(list(Path(f'/proc/{w.pid}/task').iterdir()))==1
            ready[k]=fields
            workers[k]=w
        assert all(r==ready['a'] for r in ready.values()), ready
        before=snapshot(cpu)
        reps=conf['repeats']; warmup={k: request(workers[k],reps) for k in labels}
        order = [rng.sample([0,1]*(pairs//2),pairs) for _ in comparisons]
        for i in range(pairs):
            for j in rng.sample(range(len(comparisons)),len(comparisons)):
                comp=comparisons[j]; first=order[j][i]; samples=[None,None]
                for pos in (first,1-first): samples[pos]=measure(workers[comp[pos]],reps)
                runs.append({'round':i,'comparison':comp,'first':first,'samples':samples})
        after=snapshot(cpu)
        for w in workers.values(): w.stdin.close(); assert w.wait(timeout=10)==0
    return {**conf,'argv':argv,'verify':verify,'ready':ready,'warmup':warmup,'before':before,'after':after,
            'comparisons':comparisons,'pairs':runs}

def main():
    p=argparse.ArgumentParser(); p.add_argument('mode',choices=('kernel','replay')); p.add_argument('out',type=Path)
    p.add_argument('--seed',type=int,required=True); p.add_argument('--only',nargs='+'); p.add_argument('--winner',choices=('deposit','avx','avx2'))
    p.add_argument('--pairs',type=int,default=24); p.add_argument('--cpu',type=int,default=3)
    a=p.parse_args(); assert a.pairs%2==0
    a.out.mkdir(exist_ok=True); rng=random.Random(a.seed)
    mask=['taskset','-c',str(a.cpu)]
    fixtures=ROOT/'.test-agent/decode-pool/fixtures'
    meta={'seed':a.seed,'pin':PIN,'c_pin':CPIN,'cpu':a.cpu,'mode':a.mode,'cases':{}}
    if a.mode=='kernel':
        configs=json.loads((S/'runs/meta.json').read_text()); names=a.only or list(configs)
        trial=a.winner or 'avx'
        bins={k:S/f'kernel-{k}' for k in ('a','deposit',trial,'c')}; bins['aa']=bins['a']
        bins['cp']=bins['c']
        def args_for(name):
            conf=configs[name]; reps=max(1,min(50000,24_000_000//conf['units'])); conf['repeats']=reps
            comp=[['a','deposit'],['a',trial],['deposit',trial],['a','aa']]
            if name.startswith(('single-','mozilla-','sao-')): comp += [['a','c'],['a','cp']]
            used={x for c in comp for x in c}
            argv={k:[*mask,str(bin),*(['padded' if k=='cp' else 'bounded'] if k in ('c','cp') else []),
                     'serve',str(S/'runs'/f'{name}.bin')] for k,bin in bins.items() if k in used}
            return conf,comp,argv
    else:
        assert a.winner
        names=a.only or [f'{x}-{k}' for x in REPEATS for k in ('block','lit','tok')]
        man=json.loads(MANIFEST.read_text()); arcs={x['file'][:-4]:x for x in man['members']}
        arcs['single']={**man['single'],**{f'plain_{k}':v for k,v in man['plain'].items()}}
        bins={'a':S/'replay-a','aa':S/'replay-a',a.winner:S/f'replay-{a.winner}',
              'c': ROOT/'.test-agent/decode-compact-sol/build/replay-c'}
        def args_for(name):
            fixture,kind=name.split('-'); arc=fixtures/f'{fixture}.zxc'; m=arcs[fixture]
            assert all(m[k]==v for k,v in identity(arc).items()); blocks,cases=capture(arc,a.out)
            ix=['block','lit','tok'].index(kind); args=[str(arc)]
            if kind=='block':
                conf={'count':blocks,'units':m['plain_bytes'],'expected':{k:m['plain_'+k] for k in ('bytes','sha256')}}
            else:
                conf=cases[kind]; args.append(conf['index']['path'])
            conf['repeats']=REPEATS[fixture][ix]
            comp=[['a',a.winner],['a','aa'],['a','c']]
            argv={k:[*mask,str(b),'serve',*args] for k,b in bins.items()}
            return conf,comp,argv
    meta['engine_paths']={k:{'path':str(x),**identity(x)} for k,x in bins.items()}
    rng.shuffle(names)
    with (ROOT/'.test-agent/zxc-perf.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        meta['initial']=snapshot(a.cpu)
        for name in names:
            meta['cases'][name]=run_case(name,*args_for(name),a.cpu,rng,a.pairs,a.out)
            print(name,'complete',meta['cases'][name]['after']['load'],flush=True)
            with gzip.open(a.out/'raw.json.gz','wt') as f: json.dump(meta,f)
            report(meta,a.out/'summary.tsv')
        meta['final']=snapshot(a.cpu)
    with gzip.open(a.out/'raw.json.gz','wt') as f: json.dump(meta,f)

if __name__=='__main__': main()
