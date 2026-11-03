/* Optional scalar public-API benchmark; never linked or vendored into Cargo. */
#define _POSIX_C_SOURCE 200809L
#include "opus.h"
#include "arch.h"
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#if !OPUS_FAST_INT64 || !defined(FIXED_POINT) || !defined(DISABLE_FLOAT_API)
#error "Expected scalar fixed fast64 reference without floating analysis"
#endif
static struct { unsigned char header[32]; int rate,channels,n,frames,cap,application,complexity,mode,bitrate,loss,fec,dtx,skip; opus_int16 *pcm; } input;
typedef struct {uint64_t packets,bytes,fingerprint,modes[3];} Stats;
static void fail(const char *message){fprintf(stderr,"%s\n",message);exit(1);}
static uint64_t now(void){struct timespec t;if(clock_gettime(CLOCK_MONOTONIC,&t))fail("clock failed");return (uint64_t)t.tv_sec*1000000000+(uint64_t)t.tv_nsec;}
static uint32_t u16(const unsigned char *p){return p[0]|(uint32_t)p[1]<<8;}
static uint32_t u32(const unsigned char *p){return u16(p)|u16(p+2)<<16;}
static uint64_t u64(const unsigned char *p){return u32(p)|(uint64_t)u32(p+4)<<32;}
static void put(unsigned char *p,uint64_t v,int n){for(int i=0;i<n;i++)p[i]=(unsigned char)(v>>(8*i));}
static void fold(uint64_t *h,const unsigned char *p,size_t n){for(size_t i=0;i<n;i++){*h^=p[i];*h*=UINT64_C(1099511628211);}}
static uint64_t pcm_hash(const opus_int16 *pcm,int n){uint64_t h=UINT64_C(14695981039346656037);for(int i=0;i<n;i++){unsigned char b[2];put(b,(uint16_t)pcm[i],2);fold(&h,b,2);}return h;}
static void load(const char *path){
 FILE *file=fopen(path,"rb");if(!file)fail("corpus open failed");
 if(fread(input.header,1,32,file)!=32||memcmp(input.header,"OPURAB01",8))fail("invalid corpus header");
 input.rate=u32(input.header+8);input.channels=u16(input.header+12);input.n=u16(input.header+14);input.bitrate=u32(input.header+16);input.cap=u16(input.header+20);
 int apps[]={OPUS_APPLICATION_AUDIO,OPUS_APPLICATION_VOIP,OPUS_APPLICATION_RESTRICTED_LOWDELAY,OPUS_APPLICATION_RESTRICTED_SILK,OPUS_APPLICATION_RESTRICTED_CELT};
 if(input.header[22]>4)fail("invalid application");input.application=apps[input.header[22]];
 input.complexity=input.header[23];input.mode=input.header[24];input.loss=input.header[25];input.fec=input.header[26];input.dtx=input.header[27];input.frames=u32(input.header+28);
 if((input.rate!=8000&&input.rate!=12000&&input.rate!=16000&&input.rate!=24000&&input.rate!=48000)||(input.channels!=1&&input.channels!=2)||input.frames<1||input.frames>100000||input.bitrate<1||input.bitrate>1500000||input.cap<1||input.complexity>10||input.mode>2||input.loss>100||input.fec>1||input.dtx>1)fail("invalid configuration");
 int n48=input.n*(48000/input.rate),valid=0;int sizes[]={120,240,480,960,1920,2880,3840,4800,5760};for(int i=0;i<9;i++)valid|=n48==sizes[i];
 if(!valid||input.cap>(n48>960?7656:1276))fail("invalid frame/cap");
 uint64_t samples=(uint64_t)input.frames*input.n*input.channels;if(samples*2+32>64*1024*1024)fail("corpus exceeds bound");
 input.pcm=malloc((size_t)samples*sizeof(*input.pcm));if(!input.pcm)fail("PCM allocation failed");
 if(fread(input.pcm,2,(size_t)samples,file)!=(size_t)samples)fail("PCM count mismatch");
 for(uint64_t i=0;i<samples;i++){unsigned char *p=(unsigned char*)input.pcm+2*i;input.pcm[i]=(opus_int16)u16(p);}
 if(fgetc(file)!=EOF||ferror(file)||fclose(file))fail("trailing/failed corpus input");
}
static void configure(OpusEncoder *encoder){
 if(opus_encoder_ctl(encoder,OPUS_SET_BITRATE(input.bitrate))||opus_encoder_ctl(encoder,OPUS_SET_COMPLEXITY(input.complexity))||opus_encoder_ctl(encoder,OPUS_SET_VBR(input.mode!=0))||opus_encoder_ctl(encoder,OPUS_SET_VBR_CONSTRAINT(input.mode==2))||opus_encoder_ctl(encoder,OPUS_SET_PACKET_LOSS_PERC(input.loss))||opus_encoder_ctl(encoder,OPUS_SET_INBAND_FEC(input.fec))||opus_encoder_ctl(encoder,OPUS_SET_DTX(input.dtx))||opus_encoder_ctl(encoder,OPUS_GET_LOOKAHEAD(&input.skip)))fail("native configuration failed");
}
static void record_header(unsigned char *h){memcpy(h,"OPURAE01",8);memcpy(h+8,input.header+8,8);put(h+16,input.frames,4);put(h+20,input.skip,4);}
static Stats replay(OpusEncoder *encoder,int check,FILE *record,uint64_t *latency){
 Stats stats={0,0,UINT64_C(14695981039346656037),{0,0,0}};unsigned char packet[7656];opus_int16 pcm[11520];int error=0;
 OpusDecoder *decoder=check?opus_decoder_create(input.rate,input.channels,&error):NULL;if(check&&(!decoder||error))fail("decoder init failed");
 for(int i=0;i<input.frames;i++){
  uint64_t begin=latency?now():0;
  int length=opus_encode(encoder,input.pcm+(size_t)i*input.n*input.channels,input.n,packet,input.cap);
  if(length<1||length>input.cap)fail("native encode failed");
  __asm__ __volatile__("" : : "r"(packet),"r"(length) : "memory");
  if(latency)latency[i]=now()-begin;
  if(check){
   opus_uint32 range,decoded_range;if(opus_encoder_ctl(encoder,OPUS_GET_FINAL_RANGE(&range)))fail("encoder range failed");
   int samples=opus_decode(decoder,packet,length,pcm,input.n,0);
   if(samples!=input.n||opus_decoder_ctl(decoder,OPUS_GET_FINAL_RANGE(&decoded_range))||range!=decoded_range)fail("encoder/decoder range or duration mismatch");
   unsigned char h[14];put(h,length,2);put(h+2,range,4);put(h+6,pcm_hash(pcm,samples*input.channels),8);
   fold(&stats.fingerprint,h,14);fold(&stats.fingerprint,packet,length);
   if(record&&(fwrite(h,1,14,record)!=14||fwrite(packet,1,length,record)!=(size_t)length))fail("record write failed");
   int mode=packet[0]&128?2:(packet[0]&96)==96?1:0;stats.modes[mode]++;
  }
  stats.packets++;stats.bytes+=length;
 }
 if(decoder)opus_decoder_destroy(decoder);return stats;
}
static void verify(const char *path){
 FILE *file=fopen(path,"rb");if(!file)fail("record open failed");unsigned char h[24],expected[24];record_header(expected);
 if(fread(h,1,24,file)!=24||memcmp(h,expected,24))fail("validation geometry/lookahead mismatch");
 uint64_t bands[5]={0},wire[2]={0},lbrr=0,empty=0,empty_packets=0,one_byte=0;
 int error;OpusDecoder *decoder=opus_decoder_create(input.rate,input.channels,&error);if(!decoder||error)fail("decoder init failed");
 for(int i=0;i<input.frames;i++){
  unsigned char header[14],packet[7656];opus_int16 pcm[11520];if(fread(header,1,14,file)!=14)fail("truncated validation record");int n=u16(header);if(n<1||n>input.cap||fread(packet,1,n,file)!=(size_t)n)fail("invalid validation packet");
  int samples=opus_decode(decoder,packet,n,pcm,input.n,0);opus_uint32 range;
  if(samples!=input.n||opus_decoder_ctl(decoder,OPUS_GET_FINAL_RANGE(&range))||range!=u32(header+2)||pcm_hash(pcm,samples*input.channels)!=u64(header+6))fail("cross-decoder verification failed");
  int band=opus_packet_get_bandwidth(packet)-OPUS_BANDWIDTH_NARROWBAND;
  int backup=opus_packet_has_lbrr(packet,n);if(band<0||band>4||backup<0)fail("packet diagnostics failed");bands[band]++;wire[(packet[0]>>2)&1]++;lbrr+=backup!=0;
  const unsigned char *frames[48];opus_int16 sizes[48];unsigned char toc;int offset,count=opus_packet_parse(packet,n,&toc,frames,sizes,&offset),has_empty=0;
  if(count<1)fail("packet parse failed");for(int j=0;j<count;j++){empty+=sizes[j]==0;one_byte+=sizes[j]==1;has_empty|=sizes[j]==0;}empty_packets+=has_empty;

 }
 if(fgetc(file)!=EOF||ferror(file)||fclose(file))fail("trailing/failed validation data");opus_decoder_destroy(decoder);printf("{\"verified_packets\":%d,\"bandwidth_packets\":[%"PRIu64",%"PRIu64",%"PRIu64",%"PRIu64",%"PRIu64"],\"wire_channel_packets\":[%"PRIu64",%"PRIu64"],\"lbrr_packets\":%"PRIu64",\"empty_frames\":%"PRIu64",\"packets_with_empty_frame\":%"PRIu64",\"one_byte_frames\":%"PRIu64"}\n",input.frames,bands[0],bands[1],bands[2],bands[3],bands[4],wire[0],wire[1],lbrr,empty,empty_packets,one_byte);
}
static int order(const void *a,const void *b){uint64_t x=*(const uint64_t*)a,y=*(const uint64_t*)b;return(x>y)-(x<y);}
static int number(const char *p,int maximum){char *end;long n=strtol(p,&end,10);if(!*p||*end||n<1||n>maximum)fail("invalid pass/warmup count");return(int)n;}
int main(int argc,char **argv){
 if(argc!=5&&!(argc==4&&!strcmp(argv[1],"verify")))fail("usage: encoder CORPUS RECORD_OR_DASH PASSES WARMUP; or verify CORPUS RECORD");
 int verification=argc==4;load(argv[verification?2:1]);int error;uint64_t begin=now();OpusEncoder *encoder=opus_encoder_create(input.rate,input.channels,input.application,&error);uint64_t init=now()-begin;if(!encoder||error)fail("encoder init failed");configure(encoder);
 if(verification){verify(argv[3]);opus_encoder_destroy(encoder);free(input.pcm);return 0;}
 int passes=number(argv[3],1000),warmup=number(argv[4],100);FILE *record=NULL;
 if(strcmp(argv[2],"-")){record=fopen(argv[2],"wbx");if(!record)fail("record create failed");unsigned char h[24];record_header(h);if(fwrite(h,1,24,record)!=24)fail("record header write failed");}
 Stats validation=replay(encoder,1,record,NULL);if(record&&fclose(record))fail("record close failed");
 for(int i=0;i<warmup;i++){if(opus_encoder_ctl(encoder,OPUS_RESET_STATE))fail("reset failed");replay(encoder,0,NULL,NULL);}
 uint64_t elapsed=0;
 for(int i=0;i<passes;i++){if(opus_encoder_ctl(encoder,OPUS_RESET_STATE))fail("reset failed");begin=now();Stats s=replay(encoder,0,NULL,NULL);elapsed+=now()-begin;if(s.packets!=validation.packets||s.bytes!=validation.bytes)fail("timed pass changed packet/byte counts");}
 uint64_t *latency=malloc((size_t)input.frames*sizeof(*latency));if(!latency)fail("latency allocation failed");if(opus_encoder_ctl(encoder,OPUS_RESET_STATE))fail("reset failed");replay(encoder,0,NULL,latency);
 if(opus_encoder_ctl(encoder,OPUS_RESET_STATE))fail("reset failed");Stats after=replay(encoder,1,NULL,NULL);if(after.packets!=validation.packets||after.bytes!=validation.bytes||after.fingerprint!=validation.fingerprint||memcmp(after.modes,validation.modes,sizeof(after.modes)))fail("post-timing validation changed output");
 qsort(latency,input.frames,sizeof(*latency),order);
 printf("{\"packets\":%"PRIu64",\"samples_per_pass\":%"PRIu64",\"packet_bytes\":%"PRIu64",\"fingerprint\":\"%016"PRIx64"\",\"encoder_object_bytes\":%d,\"lookahead\":%d,\"passes\":%d,\"elapsed_ns\":%"PRIu64",\"init_ns\":%"PRIu64",\"latency_ns\":{\"p50\":%"PRIu64",\"p95\":%"PRIu64",\"p99\":%"PRIu64",\"max\":%"PRIu64"},\"mode_packets\":[%"PRIu64",%"PRIu64",%"PRIu64"]}\n",validation.packets,(uint64_t)input.frames*input.n,validation.bytes,validation.fingerprint,opus_encoder_get_size(input.channels),input.skip,passes,elapsed,init,latency[(input.frames-1)*50/100],latency[(input.frames-1)*95/100],latency[(input.frames-1)*99/100],latency[input.frames-1],validation.modes[0],validation.modes[1],validation.modes[2]);
 free(latency);opus_encoder_destroy(encoder);free(input.pcm);return 0;
}
