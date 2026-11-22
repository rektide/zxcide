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

static struct result pass(zxc_cctx_t *ctx, const uint8_t *src, const struct block *blocks,
                          size_t count, uint8_t *out, size_t room, int emit) {
    struct result r = {0, 0};
    for (size_t i = 0; i < count; ++i) {
        const struct block b = blocks[i];
        int len = zxc_decompress_chunk_wrapper(ctx, src + b.off, b.len, out, room, i);
        CHECK(len > 0);
        size_t n = (size_t)len;
        r.bytes += n;
        uint64_t word = out[0] | (uint64_t)out[n / 2] << 8 | (uint64_t)out[n - 1] << 16;
        r.tag = (r.tag ^ word ^ n) * UINT64_C(0x9e3779b97f4a7c15);
        if (emit) CHECK(fwrite(out, 1, n, stdout) == n);
    }
    return r;
}

int main(int argc, char **argv) {
    CHECK(argc == 3);
    int emit = !strcmp(argv[1], "verify");
    CHECK(emit || !strcmp(argv[1], "serve"));
    size_t len, limit;
    uint8_t *src = load(argv[2], &len);
    int check, dict, seek;
    CHECK(zxc_read_file_header(src, len, &limit, &check, &dict, NULL, &seek) == ZXC_OK);
    CHECK(check && !dict && !seek);
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
    if (emit) {
        CHECK(fflush(stdout) == 0);
    } else {
        printf("ready %zu %zu %" PRIu64 " bad-checksum-rejected\n", count, expected.bytes, expected.tag);
        fflush(stdout);
        char line[32];
        while (fgets(line, sizeof(line), stdin)) {
            size_t repeats = strtoul(line, NULL, 10);
            CHECK(repeats > 0);
#ifdef REPLAY_STAGES
            memset(replay_ns, 0, sizeof(replay_ns));
#endif
            uint64_t cpu = ns(CLOCK_THREAD_CPUTIME_ID), wall = ns(CLOCK_MONOTONIC);
            for (size_t rep = 0; rep < repeats; ++rep) {
                struct result r = pass(&ctx, src, blocks, count, out, room, 0);
                CHECK(r.bytes == expected.bytes && r.tag == expected.tag);
            }
            wall = ns(CLOCK_MONOTONIC) - wall;
            cpu = ns(CLOCK_THREAD_CPUTIME_ID) - cpu;
            printf("%" PRIu64 " %" PRIu64, wall, cpu);
#ifdef REPLAY_STAGES
            for (size_t j = 0; j < 3; ++j) printf(" %" PRIu64, replay_ns[j]);
#endif
            puts("");
            fflush(stdout);
        }
    }
    zxc_cctx_free(&ctx);
    free(src);
    free(blocks);
}
