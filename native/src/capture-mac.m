#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <AppKit/AppKit.h>
#include <gst/app/gstappsrc.h>
@interface PulseCapture : NSObject <SCStreamOutput, SCStreamDelegate>
@property SCStream *stream;
@property GstElement *video;
@property GstElement *audio;
@property double firstTime;
@property BOOL stopped;
@end
@implementation PulseCapture
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error { self.stopped=YES; if(self.video)gst_app_src_end_of_stream(GST_APP_SRC(self.video)); }
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample ofType:(SCStreamOutputType)type {
    if(self.stopped || !CMSampleBufferIsValid(sample))return;
    double timestamp=CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample));
    @synchronized(self){if(self.firstTime<0)self.firstTime=timestamp;}
    GstBuffer *buffer=NULL;GstElement *source=NULL;
    if(type==SCStreamOutputTypeScreen){
        CVPixelBufferRef pixel=CMSampleBufferGetImageBuffer(sample);if(!pixel)return;
        CVPixelBufferLockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly);
        size_t width=CVPixelBufferGetWidth(pixel),height=CVPixelBufferGetHeight(pixel);
        if(CVPixelBufferGetPlaneCount(pixel)!=2){CVPixelBufferUnlockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly);return;}
        buffer=gst_buffer_new_allocate(NULL,width*height*3/2,NULL);GstMapInfo map;gst_buffer_map(buffer,&map,GST_MAP_WRITE);
        size_t offset=0;for(size_t plane=0;plane<2;plane++){uint8_t *bytes=CVPixelBufferGetBaseAddressOfPlane(pixel,plane);size_t stride=CVPixelBufferGetBytesPerRowOfPlane(pixel,plane);size_t rows=plane==0?height:height/2;for(size_t row=0;row<rows;row++){memcpy(map.data+offset,bytes+row*stride,width);offset+=width;}}
        gst_buffer_unmap(buffer,&map);CVPixelBufferUnlockBaseAddress(pixel,kCVPixelBufferLock_ReadOnly);
        GST_BUFFER_DURATION(buffer)=GST_SECOND/60;source=self.video;
    }else if(type==SCStreamOutputTypeAudio && self.audio){
        CMBlockBufferRef block=NULL;size_t size=0;
        CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sample,&size,NULL,0,NULL,NULL,0,NULL);
        AudioBufferList *list=malloc(size);
        if(CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sample,NULL,list,size,NULL,NULL,kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,&block)==noErr){
            size_t count=CMSampleBufferGetNumSamples(sample);buffer=gst_buffer_new_allocate(NULL,count*2*sizeof(float),NULL);GstMapInfo map;gst_buffer_map(buffer,&map,GST_MAP_WRITE);float *out=(float*)map.data;
            if(list->mNumberBuffers==2){float *left=list->mBuffers[0].mData,*right=list->mBuffers[1].mData;for(size_t i=0;i<count;i++){out[i*2]=left[i];out[i*2+1]=right[i];}}
            else if(list->mNumberBuffers==1 && list->mBuffers[0].mDataByteSize>=count*2*sizeof(float)){memcpy(out,list->mBuffers[0].mData,count*2*sizeof(float));}
            else memset(out,0,count*2*sizeof(float));
            gst_buffer_unmap(buffer,&map);GST_BUFFER_DURATION(buffer)=gst_util_uint64_scale(count,GST_SECOND,48000);source=self.audio;
        }
        if(block)CFRelease(block);free(list);
    }
    if(buffer&&source){GST_BUFFER_PTS(buffer)=(GstClockTime)(MAX(0,timestamp-self.firstTime)*GST_SECOND);gst_app_src_push_buffer(GST_APP_SRC(source),buffer);}
}
@end
void *pulse_capture_start(GstElement *video,GstElement *audio){
    @autoreleasepool {
        dispatch_semaphore_t ready=dispatch_semaphore_create(0);__block SCShareableContent *content=nil;
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES completionHandler:^(SCShareableContent *value,NSError *error){content=value;dispatch_semaphore_signal(ready);}];
        if(dispatch_semaphore_wait(ready,dispatch_time(DISPATCH_TIME_NOW,30*NSEC_PER_SEC))!=0 || !content.displays.count)return NULL;
        SCDisplay *display=content.displays.firstObject;for(SCDisplay *candidate in content.displays)if(candidate.displayID==CGMainDisplayID())display=candidate;
        PulseCapture *capture=[PulseCapture new];capture.video=gst_object_ref(video);capture.audio=audio?gst_object_ref(audio):NULL;capture.firstTime=-1;
        SCContentFilter *filter=[[SCContentFilter alloc]initWithDisplay:display excludingWindows:@[]];
        SCStreamConfiguration *config=[SCStreamConfiguration new];config.width=1920;config.height=1080;config.minimumFrameInterval=CMTimeMake(1,60);config.queueDepth=3;config.pixelFormat=kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;config.showsCursor=YES;config.capturesAudio=audio!=NULL;config.sampleRate=48000;config.channelCount=2;config.excludesCurrentProcessAudio=YES;
        capture.stream=[[SCStream alloc]initWithFilter:filter configuration:config delegate:capture];NSError *error=nil;
        dispatch_queue_t queue=dispatch_queue_create("com.to-unknown.pulse.capture",DISPATCH_QUEUE_SERIAL);
        [capture.stream addStreamOutput:capture type:SCStreamOutputTypeScreen sampleHandlerQueue:queue error:&error];
        if(audio)[capture.stream addStreamOutput:capture type:SCStreamOutputTypeAudio sampleHandlerQueue:queue error:&error];
        __block BOOL started=NO;[capture.stream startCaptureWithCompletionHandler:^(NSError *error){started=error==nil;dispatch_semaphore_signal(ready);}];
        if(dispatch_semaphore_wait(ready,dispatch_time(DISPATCH_TIME_NOW,30*NSEC_PER_SEC))!=0 || !started){capture.stopped=YES;gst_object_unref(capture.video);if(capture.audio)gst_object_unref(capture.audio);return NULL;}
        return (__bridge_retained void*)capture;
    }
}
void pulse_capture_stop(void *pointer){if(!pointer)return;PulseCapture *capture=(__bridge_transfer PulseCapture*)pointer;capture.stopped=YES;dispatch_semaphore_t stopped=dispatch_semaphore_create(0);[capture.stream stopCaptureWithCompletionHandler:^(NSError *error){dispatch_semaphore_signal(stopped);}];dispatch_semaphore_wait(stopped,dispatch_time(DISPATCH_TIME_NOW,5*NSEC_PER_SEC));gst_object_unref(capture.video);if(capture.audio)gst_object_unref(capture.audio);}
