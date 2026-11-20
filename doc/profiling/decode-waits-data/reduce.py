#!/usr/bin/env python3
"""Validate ZXCWAIT1 lanes and integrate half-open intervals on one monotonic clock."""
import argparse
from collections import Counter, defaultdict
import csv
from itertools import pairwise
from pathlib import Path
import struct

NONE = (1 << 64) - 1
OPS = ('bad Pool Joined Pop Full Next NextDone JobSend JobSent ResultRecv ResultGot '
       'Pending Commit Credit Terminal Entry JobRecv JobGot Compute Computed ResultSend ResultSent Exit').split()


def need(condition, what):
    if not condition:
        raise ValueError(what)


def read_trace(path):
    data = path.read_bytes()
    need(data.startswith(b'ZXCWAIT1'), 'trace magic')
    off = 8

    def take(fmt):
        nonlocal off
        values = struct.unpack_from('<' + fmt, data, off)
        off += struct.calcsize('<' + fmt)
        return values

    def lane():
        n, = take('I')
        need(0 < n <= 2048, 'lane size')
        result = []
        for _ in range(n):
            t, i, op, aux = take('QQII')
            need(0 < op < len(OPS), 'event opcode')
            result.append((t, OPS[op], i, aux))
        need(all(a[0] <= b[0] for a, b in pairwise(result)), 'lane time reversed')
        return result

    done, count = take('QI')
    need(0 < count <= 256, 'frame count')
    frames = []
    for _ in range(count):
        start, footer, end, n, m = take('QQQII')
        need(n == m <= 2048, 'input/output count mismatch')
        meta = [take('I')[0] for _ in range(n)]
        plain = [take('I')[0] for _ in range(n)]
        w, = take('I')
        need(1 <= w <= 256, 'worker count')
        frames.append((start, footer, end, list(zip(meta, plain)), lane(), [lane() for _ in range(w)]))
    need(off == len(data), 'trailing sidecar bytes')
    need(all(a[2] <= b[0] for a, b in pairwise(frames)) and frames[-1][2] <= done, 'frame overlap')
    return done, frames


class Cursor:
    def __init__(self, lane):
        self.lane = iter(lane)
        self.row = next(self.lane, None)

    def is_(self, op):
        return self.row is not None and self.row[1] == op

    def pop(self, op, idx=None, aux=0):
        need(self.is_(op), f'expected {op}, got {self.row}')
        t, _, i, x = self.row
        need((idx is None or i == idx) and (aux is None or x == aux), f'bad index/status on {self.row}; expected {idx}/{aux}')
        self.row = next(self.lane, None)
        return t, i, x


def reduce_frame(frame):
    frame_start, footer, frame_end, meta, coord, workers = frame
    n, w = len(meta), len(workers)
    c = Cursor(coord)
    pool, k, _ = c.pop('Pool')
    need(1 <= k <= 256, 'invalid slots')
    # Deltas apply as a group at a shared timestamp, even across lanes. Head
    # intervals use a separate additive key so adjacent receives are safe.
    deltas, head_edges = defaultdict(Counter), defaultdict(Counter)

    def delta(t, key, value=1):
        deltas[t][key] += value

    def span(a, b, key):
        need(a <= b, f'inverted {key}: {a} > {b}')
        delta(a, key)
        delta(b, key, -1)

    jobs = [{'index': i, 'kind': m >> 24, 'payload_bytes': m & 0xffffff, 'plain_bytes': p}
            for i, (m, p) in enumerate(meta)]
    need(all(j['kind'] in (0, 1, 2) and j['plain_bytes'] > 0 for j in jobs), 'block metadata')
    a, b, eof, full_since = 0, 0, False, None
    pending = set()
    while not c.is_('Terminal'):
        if c.is_('Pop'):
            need(not eof and a - b < k, 'pop without credit or after EOF')
            t, _, _ = c.pop('Pop', a)
            start, _, _ = c.pop('Next', a)
            end, _, _ = c.pop('NextDone', a, int(a < n))
            span(start, end, 'next' if a < n else 'eof_next')
            if a == n:
                span(t, end, 'eof_reservation')
                eof = True
            else:
                send, _, _ = c.pop('JobSend', a)
                sent, _, _ = c.pop('JobSent', a, 1)
                delta(end, 'read')
                span(send, sent, 'job_send')
                span(t, sent, 'reservation')
                delta(sent, 'admitted')
                jobs[a].update(admit=sent, job_send=send, next_start=start, next_end=end)
                a += 1
        elif c.is_('Full'):
            need(not eof and a - b == k and full_since is None, 'invalid free-pop failure')
            full_since, _, _ = c.pop('Full', a)
        elif c.is_('ResultRecv'):
            need(a > b and b not in pending, 'result receive without missing admitted head')
            start, _, _ = c.pop('ResultRecv', b)
            end, i, _ = c.pop('ResultGot')
            need(b <= i < a and i not in pending, 'duplicate/impossible result received')
            insert, _, _ = c.pop('Pending', i)
            jobs[i].update(result_got=end, pending=insert)
            pending.add(i)
            span(start, end, 'result_recv')
            head_edges[start][b] += 1
            head_edges[end][b] -= 1
            delta(insert, 'pending')
        elif c.is_('Commit'):
            need(b in pending, 'commit without ordered pending block')
            start, _, _ = c.pop('Commit', b)
            end, _, _ = c.pop('Credit', b)
            pending.remove(b)
            delta(start, 'pending', -1)
            span(start, end, 'commit')
            delta(end, 'committed')
            if full_since is not None:
                span(full_since, end, 'failed_pop')
                full_since = None
            jobs[b].update(commit=start, credit=end)
            b += 1
        else:
            raise ValueError(f'unexpected coordinator event {c.row}')
    terminal, _, _ = c.pop('Terminal', n, n)
    joined, _, _ = c.pop('Joined', NONE)
    need(c.row is None and eof and a == b == n and not pending and full_since is None, 'invalid terminal state')
    need(frame_start <= pool <= terminal <= joined <= footer <= frame_end, 'frame/pool scopes')
    span(terminal, joined, 'join')
    seen = set()
    for ordinal, lane in enumerate(workers):
        c = Cursor(lane)
        entry, _, _ = c.pop('Entry', NONE)
        while True:
            start, _, _ = c.pop('JobRecv', NONE)
            got, i, _ = c.pop('JobGot')
            span(start, got, 'recv')
            if i == NONE:
                need(got >= terminal, 'disconnect before terminal')
                span(start, got, 'terminal_recv')
                break
            need(i < n and i not in seen, 'duplicate/impossible worker job')
            seen.add(i)
            work, _, _ = c.pop('Compute', i)
            done, _, _ = c.pop('Computed', i, 1)
            send, _, _ = c.pop('ResultSend', i)
            sent, _, _ = c.pop('ResultSent', i, 1)
            j = jobs[i]
            need(j['job_send'] <= got and send <= j['result_got'], 'receipt before its send invocation')
            j.update(worker=ordinal, job_recv_start=start, job_got=got, compute=work, computed=done,
                     result_send=send, result_sent=sent)
            span(j['job_send'], got, 'offered')
            if j['admit'] < start:
                span(j['admit'], start, 'certainly_queued')
            span(work, done, 'compute')
            span(send, sent, 'send')
            delta(done, f'computed{i}')
        exit_, _, _ = c.pop('Exit', NONE)
        need(c.row is None and pool <= entry <= exit_ <= joined, 'worker outside pool')
        span(entry, exit_, 'live')
    need(seen == set(range(n)), 'missing worker job/result')
    first = min((j['compute'] for j in jobs), default=joined)
    last = jobs[-1]['admit'] if jobs else joined
    # One-block or delayed-start frames may have no interior. Drain still
    # begins at last admission; startup is capped there to keep a partition.
    startup_end = min(first, last)
    for t in (pool, first, last, joined, startup_end):
        deltas[t]
    need(min(deltas) == pool and max(deltas) == joined, 'out-of-pool edges')
    total = Counter(pool=joined - pool, workers=w, slots=k, blocks=n, plain_bytes=sum(p for _, p in meta),
                    payload_bytes=sum(j['payload_bytes'] for j in jobs), frame=frame_end - frame_start,
                    prologue=pool - frame_start, post_pool=footer - joined, footer=frame_end - footer)
    stages = {key: Counter() for key in ('startup', 'interior', 'drain')}
    heads = defaultdict(Counter)
    state, head, complete = Counter(), Counter(), -1

    for start, end in pairwise(sorted(deltas)):
        state.update(deltas[start])
        head.update(head_edges[start])
        for key in deltas[start]:
            if key.startswith('computed'):
                complete = max(complete, int(key[8:]))
        h = next((i for i, count in head.items() if count), None)
        dt = end - start
        s = Counter(wall=dt)
        m = state['compute']
        idle = state['recv']
        live = state['live']
        s[f'concurrency{m}'] += dt
        need(0 <= m + idle + state['send'] <= live <= w, 'invalid worker budget')
        for key in ('compute', 'recv', 'send', 'terminal_recv'):
            s[key] += state[key] * dt
        s['other_live'] += (live - m - idle - state['send']) * dt
        s['outside_live'] += (w - live) * dt
        queued = 'empty' if state['offered'] == 0 else 'nonempty' if state['certainly_queued'] else 'ambiguous'
        s[f'recv_{queued}'] += idle * dt

        occ = state['admitted'] - state['committed']
        reserved = state['reservation'] + state['eof_reservation']
        need(0 <= occ <= occ + reserved <= k and reserved in (0, 1), 'invalid credit occupancy')
        s[f'occupancy{occ}'] += dt
        depth = state['pending']
        need(0 <= depth <= occ and (h is not None) == bool(state['result_recv']), 'invalid pending or head state')
        s['pending_depth_time'] += depth * dt
        total['pending_peak'] = max(total['pending_peak'], depth)

        coord_keys = ('result_recv', 'commit', 'next', 'eof_next', 'job_send', 'join')
        stage = next((key for key in coord_keys if state[key]), 'coord_other')
        need(sum(state[key] for key in coord_keys) <= 1, 'overlapping coordinator spans')
        s[stage] += dt
        if reserved:
            s['reserved'] += dt
        if state['eof_reservation']:
            s['eof_reserved'] += dt
        if state['failed_pop']:
            need(occ == k, 'failed-pop span without full occupancy')
            s['failed_pop'] += dt
        unread = state['read'] < n
        if occ == k:
            s['full'] += dt
            if unread:
                s['full_unread'] += dt
                s['full_unread_' + ('recv' if h is not None else 'commit' if stage == 'commit' else 'other')] += dt
                if state['failed_pop']:
                    s['failed_pop_unread'] += dt
        if h is not None:
            label = 'c' if jobs[h]['computed'] <= start else 'b' if complete > h else 'a'
            s[label] += dt
            heads[h][label] += dt
            if label == 'b' and depth:
                s['b_pending'] += dt
                heads[h]['b_pending'] += dt
            if label == 'b' and occ == k and unread and idle and queued == 'empty':
                s['exposed'] += dt
                s['exposed_worker'] += idle * dt
                heads[h]['exposed'] += dt
                heads[h]['exposed_worker'] += idle * dt
                heads[h][f'exposed_idle{idle}'] += dt
                if depth:
                    s['exposed_pending'] += dt
                    s['exposed_pending_worker'] += idle * dt
                    heads[h]['exposed_pending'] += dt
        if unread and idle and queued == 'empty' and stage in ('next', 'commit'):
            s[f'supply_{stage}'] += dt
            s[f'supply_{stage}_worker'] += idle * dt
        total.update(s)
        section = 'startup' if start < startup_end else 'interior' if start < last else 'drain'
        stages[section].update(s)

    coord_keys = ('result_recv', 'commit', 'next', 'eof_next', 'job_send', 'join', 'coord_other')
    need(total['pool'] == sum(total[key] for key in coord_keys), 'coordinator budget mismatch')
    need(w * total['pool'] == sum(total[key] for key in ('compute', 'recv', 'send', 'other_live', 'outside_live')), 'worker budget mismatch')
    need(total['result_recv'] == sum(total[key] for key in ('a', 'b', 'c')), 'head partition mismatch')
    need(total['pool'] == sum(stages[s]['wall'] for s in stages), 'stage budget mismatch')
    need(total['pool'] == sum(total[f'occupancy{i}'] for i in range(k + 1)), 'credit budget mismatch')
    for stage, counts in stages.items():
        total.update({f'{stage}_{key}': value for key, value in counts.items()})
    for j in jobs:
        j.update(compute_ns=j['computed'] - j['compute'], job_handoff_ns=j['job_got'] - j['job_send'],
                 job_postsend_ns=max(0, j['job_got'] - j['admit']),
                 result_handoff_ns=j['result_got'] - j['result_send'],
                 result_postsend_ns=max(0, j['result_got'] - j['result_sent']),
                 result_collection_ns=j['result_got'] - j['computed'],
                 pending_ns=j['commit'] - j['result_got'])
    return total, jobs, heads


def tsv(path, rows):
    # Preserve identifying columns and first-seen metric order.
    keys = list(dict.fromkeys(key for row in rows for key in row))
    with path.open('w') as f:
        w = csv.DictWriter(f, keys, delimiter='\t', lineterminator='\n', restval=0)
        w.writeheader()
        w.writerows(rows)


def fixed_layout(archive, plaintext):
    """Independent wire walk for these fixed v9 single-frame fixtures."""
    layout = []
    with archive.open('rb') as f:
        h = f.read(16)
        need(h[:5] == bytes.fromhex('f52eb09c09'), 'not a v9 frame')
        block_size = 1 << h[5]
        checksum = 4 if h[6] & 128 else 0
        left = plaintext.stat().st_size
        while True:
            h = f.read(8)
            need(len(h) == 8, 'missing terminal block')
            kind, payload = h[0], int.from_bytes(h[3:7], 'little')
            if kind == 255:
                need(payload == 0 and left == 0, 'invalid EOF for expected plaintext bytes')
                break
            plain = min(block_size, left)
            need(kind in (0, 1, 2) and plain > 0, 'unexpected fixed-fixture block')
            layout.append(((kind << 24) | payload, plain))
            left -= plain
            f.seek(payload + checksum, 1)
    return layout


def batch(run, out, fixtures):
    samples = list(csv.DictReader((run / 'timings.tsv').open(), delimiter='\t'))
    budgets, block_rows, head_rows, layouts = [], [], [], {}
    for row in samples:
        if row['pair'] == '0' or not row['trace']:
            continue
        case = row['case']
        if case not in layouts:
            layouts[case] = fixed_layout(fixtures / f'{case}.zxc', fixtures / case.split('.')[0])
        done, frames = read_trace(run / 'traces' / row['trace'])
        need(len(frames) == 1 and frames[0][3] == layouts[case], f'{case}: fixture metadata mismatch')
        for idx, frame in enumerate(frames):
            label = {key: row[key] for key in ('series', 'case', 'test', 'pair', 'trace')} | {'frame_index': idx}
            try:
                metrics, jobs, heads = reduce_frame(frame)
            except ValueError as e:
                raise ValueError(f'{label}: {e}') from e
            wall = int(row['wall_ns'])
            need(wall >= done, 'internal clock exceeds CLI wall')
            budgets.append(label | dict(metrics) | {'outer_residual': wall - done,
                           'session_residual': done - metrics['frame']})
            for j in jobs:
                block_rows.append(label | j)
            for head, head_metrics in heads.items():
                head_rows.append(label | {'head': head, 'compute_ns': jobs[head]['compute_ns']} | dict(head_metrics))
    need(budgets, 'no trace samples')
    out.mkdir(parents=True, exist_ok=True)
    tsv(out / 'budgets.tsv', budgets)
    tsv(out / 'blocks.tsv', block_rows)
    tsv(out / 'heads.tsv', head_rows)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('run', type=Path)
    p.add_argument('out', type=Path)
    p.add_argument('--fixtures', type=Path, default=Path(__file__).resolve().parents[3] / '.test-agent/pivco-worker/fixtures')
    args = p.parse_args()
    batch(args.run, args.out, args.fixtures)
