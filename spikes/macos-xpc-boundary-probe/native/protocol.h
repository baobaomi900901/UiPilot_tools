#pragma once
#import <Foundation/Foundation.h>
#import <xpc/xpc.h>
#define PROBE_HOST_ID "org.uipilot.probe.xpc-boundary"
#define PROBE_SERVICE_ID "org.uipilot.probe.xpc-boundary.service"
#define PROBE_REQUEST "public-request-0123456789abcdef"
#define PROBE_PAYLOAD "uipilot-fixed-probe"
bool probe_parse(xpc_object_t value, const char *kind);
xpc_object_t probe_message(const char *kind);
void probe_fill(xpc_object_t value, const char *kind);
// Only accessed on the adapter's serial queue. Identity is never serialized.
@interface ProbeOwner : NSObject
@property(nonatomic) BOOL active;
@property(nonatomic) BOOL completed;
@property(nonatomic) double deadline;
- (instancetype)initWithDeadline:(double)deadline;
- (BOOL)acceptFrom:(ProbeOwner *)source now:(double)now;
- (void)retire;
@end
