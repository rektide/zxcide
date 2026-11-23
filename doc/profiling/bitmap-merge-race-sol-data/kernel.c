/* Appended to pinned zxc_huffman.c plus extracted bounded C loop. */
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <inttypes.h>

struct run { size_t n, l; const uint8_t *mask, *src; };
static size_t word(const uint8_t *s) {
    return (size_t)s[0] | (size_t)s[1]<<8 | (size_t)s[2]<<16 | (size_t)s[3]<<24;
}
static uint64_t ns(clockid_t clk) {
    struct timespec t;
    if (clock_gettime(clk,&t)) abort();
    return (uint64_t)t.tv_sec*1000000000 + (uint64_t)t.tv_nsec;
}
static uint64_t pass(const struct run *runs, size_t count, uint8_t *out, int emit, int padded) {
    uint64_t tag=0;
    for (size_t j=0;j<count;j++) {
        struct run run=runs[j];
        if (padded) zxc_pivco_merge(out,run.src,run.l,run.n-run.l,run.mask);
        else bounded(out,run.src,run.l,run.n-run.l,run.mask);
        uint64_t w = (uint64_t)out[0] | (uint64_t)out[run.n/2]<<8 | (uint64_t)out[run.n-1]<<16;
        tag=(tag ^ w ^ run.n)*UINT64_C(0x9e3779b97f4a7c15);
        if (emit && fwrite(out,1,run.n,stdout)!=run.n) abort();
    }
    return tag;
}
int main(int argc, char **argv) {
    if (argc!=4) return 2;
    int padded=!strcmp(argv[1],"padded"),emit=!strcmp(argv[2],"verify");
    FILE *f=fopen(argv[3],"rb");
    if (!f || fseek(f,0,SEEK_END)) abort();
    long len=ftell(f);
    if (len<=0 || fseek(f,0,SEEK_SET)) abort();
    uint8_t *data=malloc((size_t)len);
    if (!data || fread(data,1,(size_t)len,f)!=(size_t)len) abort();
    fclose(f);
    struct run *runs=0;
    size_t pos=0,count=0,max=0,units=0;
    while (pos<(size_t)len) {
        size_t n=word(data+pos), l=word(data+pos+4), m=(n+7)/8;
        runs=realloc(runs,(count+1)*sizeof(*runs));
        if (!runs) abort();
        runs[count++]=(struct run){n,l,data+pos+16,data+pos+16+m};
        pos+=16+m+n+32; units+=n;
        if (max<n) max=n;
    }
    if (pos!=(size_t)len) abort();
    uint8_t *out=calloc(max,1);
    if (!out) abort();
    uint64_t tag=pass(runs,count,out,emit,padded);
    if (!emit) {
        printf("ready %zu %zu %" PRIu64 " merge-oracle\n",count,units,tag);
        fflush(stdout);
        unsigned n;
        while (scanf("%u",&n)==1) {
            uint64_t c=ns(CLOCK_THREAD_CPUTIME_ID), t=ns(CLOCK_MONOTONIC);
            for (unsigned i=0;i<n;i++) if (pass(runs,count,out,0,padded)!=tag) abort();
            t=ns(CLOCK_MONOTONIC)-t;
            c=ns(CLOCK_THREAD_CPUTIME_ID)-c;
            printf("%" PRIu64 " %" PRIu64 "\n",t,c); fflush(stdout);
        }
    }
    free(out); free(runs); free(data);
}
