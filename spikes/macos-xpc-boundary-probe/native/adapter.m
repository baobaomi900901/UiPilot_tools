#import "adapter.h"
#import "policy.h"
#import <xpc/connection.h>
static dispatch_queue_t queue;
static NSMutableSet *sessions;
static double now(void) { return NSProcessInfo.processInfo.systemUptime; }
@interface ProbePeer : NSObject
@property(nonatomic,strong) ProbeOwner *owner;
@property(nonatomic,strong) xpc_connection_t connection;
@property(nonatomic,strong) dispatch_source_t timer;
@property(nonatomic) BOOL host;
- (void)finish:(BOOL)accepted;
- (void)begin:(BOOL)owned;
@end
static int requirement(void *context) {
    ProbePeer *p=(__bridge ProbePeer *)context;
    return xpc_connection_set_peer_team_identity_requirement(p.connection,p.host?PROBE_SERVICE_ID:PROBE_HOST_ID);
}
static void retire(void *context) { [((__bridge ProbePeer *)context).owner retire]; }
static void cancel(void *context) { xpc_connection_cancel(((__bridge ProbePeer *)context).connection); }
static void activate(void *context,bool cleanup) {
    (void)cleanup; xpc_connection_activate(((__bridge ProbePeer *)context).connection);
}
static BOOL exactPayload(xpc_object_t v) {
    size_t length=0; const void *bytes=xpc_dictionary_get_data(v,"payload",&length);
    return !strcmp(xpc_dictionary_get_string(v,"requestId"),PROBE_REQUEST) &&
        length==strlen(PROBE_PAYLOAD) && !memcmp(bytes,PROBE_PAYLOAD,length);
}
@implementation ProbePeer
- (void)finish:(BOOL)accepted {
    [_owner retire]; // Retirement always precedes cancellation/cleanup.
    if (_timer) { dispatch_source_cancel(_timer); _timer=nil; }
    if (_connection) { xpc_connection_cancel(_connection); _connection=nil; }
    [sessions removeObject:self];
    if (_host) { puts(accepted?"{\"status\":\"pong\"}":"{\"status\":\"unconfirmed\"}"); exit(accepted?0:4); }
}
- (void)begin:(BOOL)owned {
    __weak ProbePeer *weakSelf=self;
    ProbeOwner *original=_owner;
    xpc_connection_set_target_queue(_connection,queue);
    xpc_connection_set_event_handler(_connection,^(xpc_object_t event) {
        ProbePeer *p=weakSelf;
        if (!p || p.owner!=original || !original.active) return;
        // Includes interrupted, invalid, policy failures: no automatic retry.
        if (xpc_get_type(event)!=XPC_TYPE_DICTIONARY || p.host) { [p finish:NO]; return; }
        if (!probe_parse(event,"ping") || !exactPayload(event) || ![p.owner acceptFrom:original now:now()]) { [p finish:NO]; return; }
        xpc_object_t reply=xpc_dictionary_create_reply(event);
        if (!reply) { [p finish:NO]; return; }
        probe_fill(reply,"pong");
        xpc_connection_send_message(p.connection,reply);
        // Do not cancel before the queued response barrier. Timer bounds this
        // local handshake; neither barrier nor cancel proves process termination.
        xpc_connection_send_barrier(p.connection,^{
            dispatch_async(queue,^{ ProbePeer *current=weakSelf; if (current && current.owner==original) [current finish:YES]; });
        });
    });
    ProbePolicy policy={(__bridge void *)self,requirement,retire,cancel,activate};
    if (!probe_configure(policy,owned)) { [self finish:NO]; return; }
    _timer=dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER,0,0,queue);
    dispatch_source_set_timer(_timer,dispatch_time(DISPATCH_TIME_NOW,2*NSEC_PER_SEC),DISPATCH_TIME_FOREVER,0);
    dispatch_source_set_event_handler(_timer,^{
        ProbePeer *p=weakSelf; if (p && p.owner==original) [p finish:NO];
    });
    dispatch_resume(_timer);
    if (_host) {
        xpc_connection_send_message_with_reply(_connection,probe_message("ping"),queue,^(xpc_object_t reply) {
            ProbePeer *p=weakSelf;
            if (!p || p.owner!=original || !original.active) return;
            BOOL valid=probe_parse(reply,"pong") && exactPayload(reply) && [p.owner acceptFrom:original now:now()];
            [p finish:valid];
        });
    }
}
@end
static void initialize(void) {
    static dispatch_once_t once;
    dispatch_once(&once,^{ queue=dispatch_queue_create("org.uipilot.probe.handshake",DISPATCH_QUEUE_SERIAL); sessions=[NSMutableSet set]; });
}
void probe_run_host(void) {
    initialize();
    dispatch_async(queue,^{
        ProbePeer *p=[ProbePeer new]; p.host=YES; p.owner=[[ProbeOwner alloc] initWithDeadline:now()+2];
        p.connection=xpc_connection_create(PROBE_SERVICE_ID,queue);
        if (!p.connection) { puts("{\"status\":\"unavailable\"}"); exit(4); }
        [sessions addObject:p]; [p begin:YES];
    });
    dispatch_main();
}
void probe_accept_peer(xpc_connection_t peer) {
    initialize();
    // The listener borrows peer. Retaining it until the queue decides is explicit.
    dispatch_async(queue,^{
        if (sessions.count>=8) { xpc_connection_cancel(peer); return; }
        ProbePeer *p=[ProbePeer new]; p.connection=peer;
        p.owner=[[ProbeOwner alloc] initWithDeadline:now()+2];
        [sessions addObject:p]; [p begin:NO];
    });
}
