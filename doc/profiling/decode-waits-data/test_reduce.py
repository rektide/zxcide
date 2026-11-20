#!/usr/bin/env python3
"""Controlled edges, not wall-clock sleep assertions."""
import unittest
from reduce import NONE, reduce_frame


def fixture(mode):
    n = 2 if mode == 'drain' else 3

    def admit(i, t):
        return [(t, 'Pop', i, 0), (t, 'Next', i, 0), (t + 1, 'NextDone', i, 1),
                (t + 2, 'JobSend', i, 0), (t + 3, 'JobSent', i, 1)]

    def eof(t):
        return [(t, 'Pop', n, 0), (t, 'Next', n, 0), (t + 1, 'NextDone', n, 0)]

    def recv(head, i, t, u):
        return [(t, 'ResultRecv', head, 0), (u, 'ResultGot', i, 0), (u, 'Pending', i, 0)]

    def commit(i, t, u):
        return [(t, 'Commit', i, 0), (u, 'Credit', i, 0)]

    def worker(i, receive, start, done, send, sent):
        return [(receive, 'JobRecv', NONE, 0), (start, 'JobGot', i, 0), (start, 'Compute', i, 0),
                (done, 'Computed', i, 1), (send, 'ResultSend', i, 0), (sent, 'ResultSent', i, 1)]

    coord = [(0, 'Pool', 2, 0)] + admit(0, 1) + admit(1, 4) + [(8, 'Full', 2, 0)]
    lane0 = [(0, 'Entry', NONE, 0)] + worker(0, 1, 4, 10 if mode == 'write' else 19, 19, 20)
    lane1 = [(0, 'Entry', NONE, 0)]
    if mode == 'in-order':
        coord += recv(0, 0, 8, 20) + commit(0, 20, 21) + admit(2, 21) + [(24, 'Full', 3, 0)]
        coord += recv(1, 1, 24, 26) + commit(1, 26, 28) + eof(28) + recv(2, 2, 29, 32) + commit(2, 32, 33)
        lane0 += worker(2, 20, 24, 31, 31, 32)
        lane1 += worker(1, 1, 7, 25, 25, 26)
    else:
        coord += recv(0, 1, 8, 12) + recv(0, 0, 12, 20) + commit(0, 20, 22) + commit(1, 22, 23)
        lane1 += worker(1, 1, 7, 11, 11, 12)
        if n == 3:
            coord += admit(2, 23) + eof(26) + recv(2, 2, 27, 30) + commit(2, 30, 33)
            lane1 += worker(2, 12, 26, 29, 29, 30)
        else:
            coord += eof(23)
    coord += [(40, 'Terminal', n, n), (43, 'Joined', NONE, 0)]
    for lane, finish in ((lane0, 41), (lane1, 42)):
        lane += [(lane[-1][0], 'JobRecv', NONE, 0), (finish, 'JobGot', NONE, 0), (finish + 1, 'Exit', NONE, 0)]
    return (0, 44, 45, [(4, 4)] * n, coord, [lane0, lane1])


class Tests(unittest.TestCase):
    def test_blocked_head(self):
        budget, jobs, heads = reduce_frame(fixture('blocked'))
        self.assertEqual(heads[0]['a'], 3)
        self.assertEqual(heads[0]['b'], 8)
        self.assertEqual(heads[0]['c'], 1)
        self.assertEqual(budget['interior_exposed_pending'], 7)
        self.assertEqual(budget['interior_exposed_pending_worker'], 7)
        self.assertGreater(jobs[2]['next_start'], jobs[0]['credit'])

    def test_in_order_wait(self):
        budget, _, _ = reduce_frame(fixture('in-order'))
        self.assertGreater(budget['a'], 0)
        self.assertEqual(budget['b'], 0)
        self.assertEqual(budget['exposed'], 0)

    def test_pending_across_write_is_not_head_exposure(self):
        budget, jobs, _ = reduce_frame(fixture('write'))
        self.assertEqual(budget['b'], 0)
        self.assertEqual(budget['exposed'], 0)
        self.assertEqual(jobs[1]['pending_ns'], 10)
        self.assertEqual(jobs[0]['credit'] - jobs[0]['commit'], 2)

    def test_last_window_drain(self):
        budget, _, _ = reduce_frame(fixture('drain'))
        self.assertEqual(budget['exposed'], 0)
        self.assertEqual(budget['drain_b_pending'], 7)
        self.assertEqual(budget['interior_b_pending'], 0)
        self.assertGreater(budget['drain_recv_empty'], 0)

    def test_reject_premature_credit(self):
        frame = fixture('blocked')
        i = next(i for i, row in enumerate(frame[4]) if row[1] == 'Full')
        frame[4][i] = (8, 'Pop', 2, 0)
        with self.assertRaisesRegex(ValueError, 'pop without credit'):
            reduce_frame(frame)


if __name__ == '__main__':
    unittest.main()
