/* External libopus benchmark only. Never linked into Opura. */
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef int (*parse_fn)(const unsigned char *, int32_t, unsigned char *,
                        const unsigned char **, int16_t *, int *);
static parse_fn parse;
struct packet { unsigned char *data; uint32_t size; };
static uint64_t fingerprint(struct packet p) {
    unsigned char toc;
    const unsigned char *frames[48];
    int16_t sizes[48];
    int offset;
    int count = parse(p.data, (int32_t)p.size, &toc, frames, sizes, &offset);
    if (count < 0) return 0;
    uint64_t hash = (uint64_t)count;
    for (int i = 0; i < count; ++i) {
        hash = hash * 1000003 + (uint64_t)(frames[i] - p.data);
        hash = hash * 1000003 + (uint64_t)sizes[i];
    }
    return hash;
}
static double now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) exit(2);
    return (double)t.tv_sec * 1e9 + t.tv_nsec;
}
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    void *lib = dlopen(argv[3], RTLD_NOW);
    if (!lib) { fprintf(stderr, "%s\n", dlerror()); return 2; }
    *(void **)(&parse) = dlsym(lib, "opus_packet_parse");
    if (!parse) return 2;
    FILE *file = fopen(argv[1], "rb");
    if (!file) return 2;
    struct packet packets[65536];
    size_t count = 0;
    unsigned char length[4];
    size_t n;
    while ((n = fread(length, 1, 4, file))) {
        if (n != 4 || count == 65536) return 2;
        uint32_t size = (uint32_t)length[0] | (uint32_t)length[1]<<8 |
                        (uint32_t)length[2]<<16 | (uint32_t)length[3]<<24;
        if (size > 10000000) return 2;
        unsigned char *data = malloc(size ? size : 1);
        if (!data || fread(data, 1, size, file) != size) return 2;
        packets[count++] = (struct packet){data, size};
    }
    if (ferror(file) || fclose(file)) return 2;
    unsigned long iterations = strtoul(argv[2], NULL, 10);
    if (!count || !iterations) return 2;
    for (size_t i = 0; i < count; ++i) printf("%" PRIu64 "\n", fingerprint(packets[i]));
    for (unsigned long j = 0; j < 1000; ++j) for (size_t i = 0; i < count; ++i) {
        uint64_t value = fingerprint(packets[i]);
        __asm__ volatile("" : : "r"(value) : "memory");
    }
    double start = now();
    for (unsigned long j = 0; j < iterations; ++j) for (size_t i = 0; i < count; ++i) {
        uint64_t value = fingerprint(packets[i]);
        __asm__ volatile("" : : "r"(value) : "memory");
    }
    printf("ns %.9f\n", (now() - start) / ((double)iterations * count));
    for (size_t i = 0; i < count; ++i) free(packets[i].data);
    dlclose(lib);
    return 0;
}
