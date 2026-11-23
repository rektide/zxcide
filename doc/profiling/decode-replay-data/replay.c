/* Diagnostic for single, checked, dictionaryless SPL16 v9 frames on Linux. */
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "zxc_internal.h"

#ifdef REPLAY_STAGES
#include "stage.h"
uint64_t replay_start, replay_ns[3];
#endif

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "failed: %s\n", #x); exit(1); } } while (0)

struct block { size_t off, len; };

static uint64_t ns(clockid_t clock) {
    struct timespec t;
    CHECK(clock_gettime(clock, &t) == 0);
    return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}

static uint8_t *load(const char *path, size_t *n) {
    FILE *f = fopen(path, "rb");
    CHECK(f && fseek(f, 0, SEEK_END) == 0);
    long len = ftell(f);
    CHECK(len >= ZXC_FILE_HEADER_SIZE && fseek(f, 0, SEEK_SET) == 0);
    *n = (size_t)len;
    uint8_t *p = malloc(*n);
    CHECK(p && fread(p, 1, *n, f) == *n && fclose(f) == 0);
    return p;
}

struct result { size_t bytes; uint64_t tag; };

static void consume(struct result *r, const uint8_t *out, size_t n, int emit) {
    r->bytes += n;
    uint64_t word = out[0] | (uint64_t)out[n / 2] << 8 | (uint64_t)out[n - 1] << 16;
    r->tag = (r->tag ^ word ^ n) * UINT64_C(0x9e3779b97f4a7c15);
    if (emit) CHECK(fwrite(out, 1, n, stdout) == n);
}

// A pass macro keeps the direct calls in each of the two serve modes.
#ifdef REPLAY_STAGES
#define RESET_STAGES() memset(replay_ns, 0, sizeof(replay_ns))
#define PRINT_STAGES() do { for (size_t j = 0; j < 3; ++j) printf(" %" PRIu64, replay_ns[j]); } while (0)
#else
#define RESET_STAGES() ((void)0)
#define PRINT_STAGES() ((void)0)
#endif
#define REPLAY_SERVE(pass_call, count, reject) do { \
    printf("ready %zu %zu %" PRIu64 " " reject "\n", count, expected.bytes, expected.tag); \
    fflush(stdout); \
    char line[32]; \
    while (fgets(line, sizeof(line), stdin)) { \
        size_t repeats = strtoul(line, NULL, 10); \
        CHECK(repeats > 0); \
        RESET_STAGES(); \
        uint64_t cpu = ns(CLOCK_THREAD_CPUTIME_ID), wall = ns(CLOCK_MONOTONIC); \
        for (size_t rep = 0; rep < repeats; ++rep) { \
            struct result r = pass_call; \
            CHECK(r.bytes == expected.bytes && r.tag == expected.tag); \
        } \
        wall = ns(CLOCK_MONOTONIC) - wall; \
        cpu = ns(CLOCK_THREAD_CPUTIME_ID) - cpu; \
        printf("%" PRIu64 " %" PRIu64, wall, cpu); \
        PRINT_STAGES(); \
        puts(""); \
        fflush(stdout); \
    } \
} while (0)

static struct result pass(zxc_cctx_t *ctx, const uint8_t *src, const struct block *blocks,
                          size_t count, uint8_t *out, size_t room, int emit) {
    struct result r = {0, 0};
    for (size_t i = 0; i < count; ++i) {
        const struct block b = blocks[i];
        int len = zxc_decompress_chunk_wrapper(ctx, src + b.off, b.len, out, room, i);
        CHECK(len > 0);
        consume(&r, out, (size_t)len, emit);
    }
    return r;
}

#ifdef REPLAY_PERF
// Bound directly to the same variant that the AVX2 SPL wrapper calls.
// zxc_huf_decode_section() without the suffix always selects the default tier.
extern int zxc_huf_decode_section_avx2(const uint8_t *, size_t, uint8_t *, size_t, uint8_t *);

struct section { size_t off, len, symbols; };

static struct result huf_pass(const uint8_t *src, const struct section *rows, size_t count,
                              uint8_t *out, uint8_t *scratch, int emit) {
    struct result r = {0, 0};
    for (size_t i = 0; i < count; ++i) {
        const struct section s = rows[i];
        CHECK(zxc_huf_decode_section_avx2(src + s.off, s.len, out, s.symbols, scratch) == ZXC_OK);
        consume(&r, out, s.symbols, emit);
    }
    return r;
}
#endif

int main(int argc, char **argv) {
#ifdef REPLAY_PERF
    int huf = argc == 4;
    CHECK(argc == 3 || huf);
#else
    CHECK(argc == 3);
#endif
    int emit = !strcmp(argv[1], "verify");
    CHECK(emit || !strcmp(argv[1], "serve"));
    size_t len, limit;
    uint8_t *src = load(argv[2], &len);
    int check, dict, seek;
    CHECK(zxc_read_file_header(src, len, &limit, &check, &dict, NULL, &seek) == ZXC_OK);
    CHECK(check && !dict && !seek);
#ifdef REPLAY_PERF
    if (huf) {
        CHECK(__builtin_cpu_supports("avx2"));
        FILE *index = fopen(argv[3], "r");
        CHECK(index);
        size_t count = 0, row_cap = 0, end = 0;
        struct section *rows = NULL, s;
        int rc;
        while ((rc = fscanf(index, "%zu %zu %zu", &s.off, &s.len, &s.symbols)) == 3) {
            CHECK(end <= s.off && s.off <= len && s.len >= 128 && s.len <= len - s.off);
            CHECK(s.symbols > 0 && s.symbols <= limit);
            end = s.off + s.len;
            if (count == row_cap) {
                row_cap = row_cap ? 2 * row_cap : 16;
                rows = realloc(rows, row_cap * sizeof(*rows));
                CHECK(rows);
            }
            rows[count++] = s;
        }
        CHECK(rc == EOF && fclose(index) == 0 && count > 0);
        uint8_t *out = malloc(limit + ZXC_PAD_SIZE), *scratch = malloc(limit + ZXC_PIVCO_SCRATCH_PAD);
        CHECK(out && scratch);
        memset(out, 0xa5, limit + ZXC_PAD_SIZE);
        memset(scratch, 0x5a, limit + ZXC_PIVCO_SCRATCH_PAD);
        uint8_t invalid[128];
        memset(invalid, 0xff, sizeof(invalid));
        CHECK(zxc_huf_decode_section_avx2(invalid, sizeof(invalid), out, 1, scratch) == ZXC_ERROR_CORRUPT_DATA);
        const struct result expected = huf_pass(src, rows, count, out, scratch, emit);
        if (!emit) REPLAY_SERVE(huf_pass(src, rows, count, out, scratch, 0), count, "bad-table-rejected");
        CHECK(fflush(stdout) == 0);
        free(scratch);
        free(out);
        free(rows);
        free(src);
        return 0;
    }
#endif
    size_t pos = ZXC_FILE_HEADER_SIZE, count = 0;
    struct block *blocks = NULL;
    for (;;) {
        zxc_block_header_t h;
        CHECK(zxc_read_block_header(src + pos, len - pos, &h) == ZXC_OK);
        if (h.block_type == ZXC_BLOCK_EOF) {
            CHECK(h.comp_size == 0);
            break;
        }
        size_t n = ZXC_BLOCK_HEADER_SIZE + (size_t)h.comp_size + ZXC_BLOCK_CHECKSUM_SIZE;
        CHECK(h.block_type == ZXC_BLOCK_SPL && h.comp_size >= 12 && h.comp_size <= limit);
        CHECK(n <= len - pos && src[pos + ZXC_BLOCK_HEADER_SIZE + 11] == 0); // SPL16
        struct block *next = realloc(blocks, sizeof(*blocks) * (count + 1));
        CHECK(next);
        blocks = next;
        blocks[count++] = (struct block){pos, n};
        pos += n;
    }
    CHECK(count > 1);
    size_t room = limit + ZXC_DECOMPRESS_TAIL_PAD;
    zxc_cctx_t ctx;
    CHECK(zxc_cctx_init(&ctx, limit, 0, 0, 1, 0) == ZXC_OK);
    ctx.compression_level = 0;
    uint8_t *out = ctx.work_buf;
    CHECK(ctx.work_buf_cap == room);
    memset(out, 0, room);

    const struct block mid = blocks[count / 2];
    src[mid.off + mid.len - 4] ^= 1;
    CHECK(zxc_decompress_chunk_wrapper(&ctx, src + mid.off, mid.len, out, room, count / 2)
          == ZXC_ERROR_BAD_CHECKSUM);
    src[mid.off + mid.len - 4] ^= 1;

    struct result expected = pass(&ctx, src, blocks, count, out, room, emit);
    if (!emit) REPLAY_SERVE(pass(&ctx, src, blocks, count, out, room, 0), count, "bad-checksum-rejected");
    CHECK(fflush(stdout) == 0);
    zxc_cctx_free(&ctx);
    free(src);
    free(blocks);
}
