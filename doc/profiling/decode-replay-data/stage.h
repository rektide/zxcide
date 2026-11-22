#ifndef DECODE_REPLAY_STAGE_H
#define DECODE_REPLAY_STAGE_H
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

extern uint64_t replay_start, replay_ns[3];

static inline uint64_t replay_now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) abort();
    return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}

static inline void replay_begin(void) { replay_start = replay_now(); }
static inline void replay_cut(unsigned stage) {
    uint64_t end = replay_now();
    replay_ns[stage] += end - replay_start;
    replay_start = end;
}
#endif
