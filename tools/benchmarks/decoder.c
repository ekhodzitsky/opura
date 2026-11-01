/* Optional scalar reference benchmark; never linked by Cargo. */
#define _POSIX_C_SOURCE 200809L
#include "opus.h"
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
typedef struct {unsigned char *data;uint32_t range,length;} Record;
typedef struct {Record *records;size_t length,capacity;} Stream;
typedef struct {uint64_t packets,samples,fingerprint;} Pass;
static Stream streams[12];
static int channels;
static void fail(const char *message) {fprintf(stderr,"%s\n",message);exit(1);}
static uint64_t now(void) {struct timespec t;if(clock_gettime(CLOCK_MONOTONIC,&t))fail("clock failed");return (uint64_t)t.tv_sec*1000000000+(uint64_t)t.tv_nsec;}
static uint32_t be32(const unsigned char *b) {return (uint32_t)b[0]<<24|(uint32_t)b[1]<<16|(uint32_t)b[2]<<8|b[3];}
static void fold(uint64_t *h,uint32_t value,int bytes) {for(int i=0;i<bytes;i++){*h^=(value>>(8*i))&255;*h*=UINT64_C(1099511628211);}}
static void load(const char *directory) {
    for(int s=0;s<12;s++) {
        char path[4096];if(snprintf(path,sizeof(path),"%s/testvector%02d.bit",directory,s+1)>=(int)sizeof(path))fail("path too long");
        FILE *file=fopen(path,"rb");if(!file)fail("vector open failed");
        unsigned char header[8];size_t count;
        uint64_t bytes=0;
        while((count=fread(header,1,8,file))) {
            if(count!=8)fail("truncated vector header");
            uint32_t length=be32(header);if(!length||length>65536)fail("invalid packet length");
            bytes+=length+8;if(bytes>256*1024*1024)fail("vector exceeds bound");
            Stream *stream=&streams[s];
            if(stream->length==stream->capacity) {
                size_t capacity=stream->capacity?stream->capacity*2:1024;
                if(capacity>1048576)fail("too many records");
                Record *records=realloc(stream->records,capacity*sizeof(*records));if(!records)fail("record allocation failed");stream->records=records;stream->capacity=capacity;
            }
            Record *r=&stream->records[stream->length++];r->length=length;r->range=be32(header+4);r->data=malloc(length);if(!r->data)fail("payload allocation failed");
            if(fread(r->data,1,length,file)!=length)fail("truncated payload");
        }
        if(ferror(file)||fclose(file)||!streams[s].length)fail("vector read failed");
    }
}
static Pass replay(OpusDecoder *decoder,int check,uint64_t *latency) {
    Pass pass={0,0,UINT64_C(14695981039346656037)};opus_int16 pcm[11520];
    for(int s=0;s<12;s++) {
        if(opus_decoder_ctl(decoder,OPUS_RESET_STATE))fail("reset failed");
        for(size_t i=0;i<streams[s].length;i++) {
            Record *r=&streams[s].records[i];uint64_t begin=latency?now():0;
            int n=opus_decode(decoder,r->data,(opus_int32)r->length,pcm,5760,0);
            if(n<=0)fail("reference decode failed");
            __asm__ __volatile__("" : : "r"(pcm) : "memory");
            if(latency)latency[pass.packets]=now()-begin;
            if(check) {
                opus_uint32 range;if(opus_decoder_ctl(decoder,OPUS_GET_FINAL_RANGE(&range)))fail("range query failed");
                if(r->range && range!=r->range)fail("encoded range mismatch");
                for(int j=0;j<n*channels;j++)fold(&pass.fingerprint,(uint16_t)pcm[j],2);
                fold(&pass.fingerprint,(uint32_t)n,4);fold(&pass.fingerprint,range,4);
            }
            pass.packets++;pass.samples+=(unsigned)n;
        }
    }return pass;
}
static int order_u64(const void *a,const void *b) {uint64_t x=*(const uint64_t*)a,y=*(const uint64_t*)b;return (x>y)-(x<y);}
int main(int argc,char **argv) {
    if(argc!=6)fail("usage: decoder VECTOR_DIR RATE CHANNELS PASSES WARMUP");
    int rate=atoi(argv[2]);channels=atoi(argv[3]);int passes=atoi(argv[4]),warmup=atoi(argv[5]);
    if((rate!=8000&&rate!=12000&&rate!=16000&&rate!=24000&&rate!=48000)||(channels!=1&&channels!=2)||passes<1||passes>1000||warmup<1||warmup>100)fail("invalid arguments");
    load(argv[1]);int error;OpusDecoder *decoder=opus_decoder_create(rate,channels,&error);if(!decoder||error)fail("decoder init failed");
    Pass validation=replay(decoder,1,NULL),observed={0};
    for(int i=0;i<warmup;i++){observed=replay(decoder,0,NULL);__asm__ __volatile__("" : : "r"(&observed) : "memory");}
    uint64_t begin=now();
    for(int i=0;i<passes;i++){observed=replay(decoder,0,NULL);__asm__ __volatile__("" : : "r"(&observed) : "memory");}
    uint64_t elapsed=now()-begin;
    uint64_t *latency=malloc(validation.packets*sizeof(*latency));if(!latency)fail("latency allocation failed");
    replay(decoder,0,latency);
    uint64_t mode_times[3]={0},mode_counts[3]={0},index=0;
    for(int s=0;s<12;s++)for(size_t i=0;i<streams[s].length;i++) {
        unsigned char toc=streams[s].records[i].data[0];int mode=toc&0x80?2:(toc&0x60)==0x60?1:0;
        mode_times[mode]+=latency[index++];mode_counts[mode]++;
    }
    qsort(latency,validation.packets,sizeof(*latency),order_u64);
    printf("{\"packets\":%"PRIu64",\"samples_per_pass\":%"PRIu64",\"fingerprint\":\"%016"PRIx64"\",\"decoder_bytes\":%d,\"passes\":%d,\"elapsed_ns\":%"PRIu64",\"latency_ns\":{\"p50\":%"PRIu64",\"p95\":%"PRIu64",\"p99\":%"PRIu64",\"max\":%"PRIu64"},\"mode_packets\":[%"PRIu64",%"PRIu64",%"PRIu64"],\"mode_latency_total_ns\":[%"PRIu64",%"PRIu64",%"PRIu64"]}\n",validation.packets,validation.samples,validation.fingerprint,opus_decoder_get_size(channels),passes,elapsed,latency[(validation.packets-1)*50/100],latency[(validation.packets-1)*95/100],latency[(validation.packets-1)*99/100],latency[validation.packets-1],mode_counts[0],mode_counts[1],mode_counts[2],mode_times[0],mode_times[1],mode_times[2]);
    free(latency);opus_decoder_destroy(decoder);
    for(int s=0;s<12;s++){for(size_t i=0;i<streams[s].length;i++)free(streams[s].records[i].data);free(streams[s].records);}return 0;
}
