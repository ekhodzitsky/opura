/* Standalone probe linked only to the nominated system libopus. */
#define _POSIX_C_SOURCE 200809L
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "opus.h"

typedef struct {
    const unsigned char *data;
    uint32_t bytes, samples, range;
} Record;

static void fail(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(EXIT_FAILURE);
}
static uint32_t be32(const unsigned char *p) {
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
}
static uint64_t now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) fail("clock");
    return (uint64_t)t.tv_sec * 1000000000 + (uint64_t)t.tv_nsec;
}
static int cmp(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}
static int replay(int rate, int channels, Record *packets, size_t count, FILE *pcm, uint64_t *times) {
    int error, wrong_ranges = 0;
    OpusDecoder *decoder = opus_decoder_create(rate, channels, &error);
    if (error != OPUS_OK || !decoder) fail("decoder creation");
    opus_int16 output[5760 * 2];
    for (size_t i = 0; i < count; i++) {
        Record r = packets[i];
        uint64_t start = now();
        int n = opus_decode(decoder, r.data, (opus_int32)r.bytes, output, (int)r.samples, 0);
        times[i] = now() - start;
        if (n != (int)r.samples) fail("decoded frame length");
        if (pcm) {
            opus_uint32 range;
            if (opus_decoder_ctl(decoder, OPUS_GET_FINAL_RANGE(&range))) fail("final range");
            wrong_ranges += r.range && range != r.range;
            for (int j = 0; j < n * channels; j++) {
                uint16_t sample = (uint16_t)output[j];
                if (fputc(sample & 255, pcm) == EOF || fputc(sample >> 8, pcm) == EOF) fail("PCM write");
            }
        }
    }
    opus_decoder_destroy(decoder);
    return wrong_ranges;
}
int main(int argc, char **argv) {
    if (argc != 5) fail("expected rate, channels, stream and PCM path or '-'");
    int rate = atoi(argv[1]), channels = atoi(argv[2]);
    if ((rate != 16000 && rate != 48000) || (channels != 1 && channels != 2)) fail("geometry");
    FILE *input = fopen(argv[3], "rb");
    if (!input || fseek(input, 0, SEEK_END)) fail("input");
    long bytes = ftell(input);
    if (bytes <= 0 || bytes > 32 * 1024 * 1024 || fseek(input, 0, SEEK_SET)) fail("stream size");
    unsigned char *data = malloc((size_t)bytes);
    Record *records = calloc((size_t)bytes / 13 + 1, sizeof(*records));
    if (!data || !records || fread(data, 1, bytes, input) != (size_t)bytes) fail("stream read");
    fclose(input);
    size_t count = 0, pos = 0;
    while (pos < (size_t)bytes) {
        if ((size_t)bytes - pos < 12) fail("stream header");
        uint32_t len = be32(data + pos), samples = be32(data + pos + 4);
        if (!len || len > (size_t)bytes - pos - 12 || !samples || samples > (unsigned)rate * 120 / 1000)
            fail("stream record");
        records[count++] = (Record){data + pos + 12, len, samples, be32(data + pos + 8)};
        pos += 12 + len;
    }
    FILE *pcm = NULL;
    uint64_t *times = calloc(count, sizeof(*times)), elapsed = 0;
    if (!times) fail("latency allocation");
    if (!strcmp(argv[4], "-")) replay(rate, channels, records, count, NULL, times);
    else if (!(pcm = fopen(argv[4], "wb"))) fail("PCM open");
    int mismatches = replay(rate, channels, records, count, pcm, times);
    if (pcm && fclose(pcm)) fail("PCM close");
    for (size_t i = 0; i < count; i++) elapsed += times[i];
    qsort(times, count, sizeof(*times), cmp);
    printf("%zu %d %" PRIu64 " %" PRIu64 "\n", count, mismatches, elapsed, times[(99 * count + 99) / 100 - 1]);
    free(times);
    free(records);
    free(data);
    return 0;
}
