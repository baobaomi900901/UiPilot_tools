#import <Foundation/Foundation.h>
#import <JavaScriptCore/JavaScriptCore.h>
#include <stdio.h>
#include <string.h>

static int emit(NSString *generation, NSString *request, NSString *status, id value) {
    NSData *data=[NSJSONSerialization dataWithJSONObject:@{@"generation":generation,@"requestId":request,@"status":status,@"value":value?:NSNull.null} options:0 error:NULL];
    if(!data || data.length>8192)return 2;
    return fwrite(data.bytes,1,data.length,stdout)==data.length ? 0 : 2;
}
int main(void) { @autoreleasepool {
    unsigned char bytes[4097]; size_t count=fread(bytes,1,sizeof(bytes),stdin);
    if(count>4096 || ferror(stdin))return 2;
    id decoded=[NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:bytes length:count] options:0 error:NULL];
    if(![decoded isKindOfClass:NSDictionary.class] || [decoded count]!=3)return 2;
    NSDictionary *input=decoded; NSString *generation=input[@"generation"], *request=input[@"requestId"], *mode=input[@"case"];
    if(![generation isKindOfClass:NSString.class] || ![request isKindOfClass:NSString.class] || ![mode isKindOfClass:NSString.class] || generation.length>20 || !generation.length || request.length>128 || !request.length)return 2;
    // Explicit synthetic transport faults, never reachable through JS/native APIs.
    if([mode isEqual:@"duplicateKeys"]){fputs("{\"generation\":\"1\",\"generation\":\"1\",\"requestId\":\"request-1\",\"status\":\"ok\",\"value\":null}",stdout);return 0;}
    if([mode isEqual:@"booleanOwner"]){fputs("{\"generation\":true,\"requestId\":\"request-1\",\"status\":\"ok\",\"value\":null}",stdout);return 0;}
    if([mode isEqual:@"extraField"]){fputs("{\"generation\":\"1\",\"requestId\":\"request-1\",\"status\":\"ok\",\"value\":null,\"extra\":true}",stdout);return 0;}
    if([mode isEqual:@"malformed"]){fputs("{",stdout);return 0;}
    if([mode isEqual:@"combinedFlood"]){
        // Each stream alone is under 8 KiB; only their combined size is too large.
        for(int i=0;i<5000;i++){fputc('x',stdout);fputc('y',stderr);}return 0;
    }
    if([mode isEqual:@"oversized"] || [mode isEqual:@"stderrFlood"]){
        FILE *stream=[mode isEqual:@"oversized"]?stdout:stderr;
        for(int i=0;i<10000;i++)fputc('x',stream);return 0;
    }
    if([mode isEqual:@"nonfinite"]){fputs("{\"generation\":\"1\",\"requestId\":\"request-1\",\"status\":\"ok\",\"value\":1e999}",stdout);return 0;}
    if([mode isEqual:@"wrongRequest"])return emit(generation,@"different-request",@"ok",@"synthetic");
    if([mode isEqual:@"stale"])return emit(@"0",request,@"ok",@"synthetic");
    if([mode isEqual:@"duplicate"]){emit(generation,request,@"ok",@"synthetic");return emit(generation,request,@"ok",@"synthetic");}
    JSContext *context=[JSContext new];
    __block BOOL failed=NO,done=NO;
    __block NSString *response=nil;
    context.exceptionHandler=^(JSContext *ctx,JSValue *exception){(void)ctx;(void)exception;failed=YES;};
    // Only a primitive string can cross this probe bridge. No JSExport/native
    // objects or process/file/network services are exposed to the interpreter.
    context[@"__finish"] = ^(JSValue *value) {
        if(failed)return;
        if(done){failed=YES;response=nil;return;}
        if(!value.isString){failed=YES;return;}
        NSString *text=value.toString;
        if(text.length>4096){failed=YES;return;}
        response=[text copy];done=YES;
    };
    NSDictionary *scripts=@{
        @"success":@"__finish(JSON.stringify({text:'synthetic',sum:2+3}));",
        @"doubleFinish":@"__finish('{}');__finish('{}');",
        @"async":@"(async()=>{await Promise.resolve();__finish(JSON.stringify({text:'async synthetic'}));})()",
        @"globals":@"__finish(JSON.stringify(Object.fromEntries(['window','document','Audio','AudioContext','webkitAudioContext','speechSynthesis','fetch','XMLHttpRequest','WebSocket','Worker','TextEncoder','URL','setTimeout','ObjC','require','process'].map(k=>[k,typeof globalThis[k]]))));",
        @"throw":@"throw new Error('PRIVATE_SYNTHETIC_ERROR_DO_NOT_EXPOSE');",
        @"pending":@"(async()=>{await new Promise(()=>{});__finish('{}');})()",
        @"infinite":@"while(true){}",
        @"finishThenSpin":@"__finish('{}');while(true){}",
        @"esm":@"export async function onCommand(){return {requestId:'fixture',results:[]};}"
    };
    NSString *script=scripts[mode];if(!script)return 2;
    @try {
        [context evaluateScript:script];
        if([mode isEqual:@"esm"])return failed?emit(generation,request,@"esmUnsupported",nil):2;
        while(!done && !failed){
            // Pending Promise is genuinely pending; only the parent can impose
            // a wall-clock deadline and terminate this process.
            @autoreleasepool { [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]]; }
        }
        if(failed)return emit(generation,request,@"scriptFailed",nil);
        NSData *data=[response dataUsingEncoding:NSUTF8StringEncoding];
        id value=data?[NSJSONSerialization JSONObjectWithData:data options:NSJSONReadingFragmentsAllowed error:NULL]:nil;
        if(!value)return emit(generation,request,@"scriptFailed",nil);
        return emit(generation,request,@"ok",value);
    } @catch(NSException *exception){(void)exception;return emit(generation,request,@"scriptFailed",nil);}
} }
