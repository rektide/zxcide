/* Single-threaded one-shot buffer API probe. Link against the frozen libzxc.a. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "zxc_buffer.h"

static unsigned char *load(const char *path, size_t *len) {
    FILE *f = fopen(path, "rb");
    if (!f || fseek(f, 0, SEEK_END)) exit(2);
    long n = ftell(f);
    if (n < 0 || fseek(f, 0, SEEK_SET)) exit(2);
    unsigned char *p = malloc((size_t)n + 1);
    if (!p || fread(p, 1, (size_t)n, f) != (size_t)n || fclose(f)) exit(2);
    *len = (size_t)n;
    return p;
}
static double now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) exit(2);
    return t.tv_sec * 1000.0 + t.tv_nsec / 1e6;
}
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    size_t as, ps;
    unsigned char *archive = load(argv[1], &as), *plain = load(argv[2], &ps), *out = malloc(ps + 1);
    if (!out) return 2;
    int n = atoi(argv[3]);
    if (n < 1) return 2;
    /* Validation and one warmup outside timed samples. */
    if (zxc_decompress(archive, as, out, ps) != (int64_t)ps || memcmp(out, plain, ps)) return 1;
    for (int i = 0; i < n; i++) {
        double start = now();
        int64_t written = zxc_decompress(archive, as, out, ps);
        double elapsed = now() - start;
        if (written != (int64_t)ps) return 1;
        printf("%.6f\n", elapsed);
    }
    if (memcmp(out, plain, ps)) return 1;
    free(archive); free(plain); free(out);
    return 0;
}
