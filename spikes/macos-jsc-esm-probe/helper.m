#import <Foundation/Foundation.h>
#import <JavaScriptCore/JavaScriptCore.h>
#include <stdio.h>
#include "fixture.h"

static NSString *source(const unsigned char *bytes, NSUInteger length) {
    return [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
}
static int emit(NSString *generation, NSString *request, NSString *status, id value) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:@{
        @"generation": generation, @"requestId": request, @"status": status,
        @"value": value ?: NSNull.null,
    } options:0 error:NULL];
    if (!data || data.length > 8192) return 2;
    return fwrite(data.bytes, 1, data.length, stdout) == data.length ? 0 : 2;
}

int main(void) { @autoreleasepool {
    unsigned char bytes[4097];
    size_t count = fread(bytes, 1, sizeof(bytes), stdin);
    if (count > 4096 || ferror(stdin)) return 2;
    id decoded = [NSJSONSerialization JSONObjectWithData:
        [NSData dataWithBytes:bytes length:count] options:0 error:NULL];
    if (![decoded isKindOfClass:NSDictionary.class] || [decoded count] != 3) return 2;
    NSDictionary *input = decoded;
    NSString *generation = input[@"generation"], *request = input[@"requestId"], *mode = input[@"case"];
    if (![generation isKindOfClass:NSString.class] || !generation.length || generation.length > 20 ||
        ![request isKindOfClass:NSString.class] || !request.length || request.length > 128 ||
        ![mode isKindOfClass:NSString.class] || ![mode isEqual:@"success"]) return 2;

    __block BOOL failed = NO, done = NO;
    __block NSString *response = nil;
    @try {
        JSContext *context = [JSContext new];
        context.exceptionHandler = ^(JSContext *ctx, JSValue *exception) {
            (void)ctx; (void)exception; failed = YES;
        };
        // Primitive-string callback, never an NSObject/JSExport API or a global.
        // Retained only by the runner's private closure before package evaluation.
        JSValue *completion = [JSValue valueWithObject:^(JSValue *value) {
            if (done) { failed = YES; response = nil; return; }
            if (failed) return;
            if (!value.isString) { failed = YES; return; }
            NSString *text = value.toString;
            if (!text.length || text.length > 4096) { failed = YES; return; }
            done = YES;
            response = [text copy];
        } inContext:context];
        NSDictionary *invocation = @{
            @"apiVersion": @1, @"requestId": request, @"input": @"中文 😀  保留空格",
            @"context": @{@"platform": @"macos", @"theme": @"dark", @"invokedAt": @"2026-11-08T12:34:56+08:00"},
        };
        NSData *invocationData = [NSJSONSerialization dataWithJSONObject:invocation options:0 error:NULL];
        NSString *invocationJSON = [[NSString alloc] initWithData:invocationData encoding:NSUTF8StringEncoding];
        JSValue *factory = [context evaluateScript:source(runner_source, sizeof(runner_source))];
        JSValue *runner = [factory callWithArguments:@[completion, invocationJSON]];
        if (failed || !runner || runner.isUndefined) return emit(generation, request, @"scriptFailed", nil);

        BOOL missingTextEncoder = context[@"TextEncoder"].isUndefined;
        // Separate evaluation / lexical scope: package does not enclose runner,
        // callback, input JSON or other host-private bindings.
        [context evaluateScript:source(bundle_source, sizeof(bundle_source))];
        if (!execute_command) {
            // Translate is a load-only observation. Do not call onCommand or
            // any permission API; do not infer an exception message/root cause.
            return emit(generation, request, @"ok", @{
                @"phase": @"moduleEvaluation", @"completed": @(!failed),
                @"textEncoderMissingBeforeLoad": @(missingTextEncoder),
            });
        }
        if (failed) return emit(generation, request, @"scriptFailed", nil);
        JSValue *handler = [context[@"UiPilotProbeModule"] valueForProperty:@"onCommand"];
        [runner callWithArguments:@[handler]];
        while (!done && !failed) {
            // Pending promises have no fabricated in-thread timeout. The parent
            // deadline terminates and waitpid-reaps this exact helper process.
            @autoreleasepool {
                [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
            }
        }
        if (failed) return emit(generation, request, @"scriptFailed", nil);
        NSData *data = [response dataUsingEncoding:NSUTF8StringEncoding];
        id value = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL] : nil;
        // Independent native response ID check in addition to runner and parent
        // transport generation/request ownership.
        if (![value isKindOfClass:NSDictionary.class] || ![value[@"requestId"] isEqual:request])
            return emit(generation, request, @"scriptFailed", nil);
        return emit(generation, request, @"ok", value);
    } @catch (NSException *exception) {
        (void)exception;
        return emit(generation, request, @"scriptFailed", nil);
    }
} }
