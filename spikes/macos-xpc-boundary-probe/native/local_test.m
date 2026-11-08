#import "protocol.h"
#import "preflight.h"
#include <assert.h>
int main(void) {
    @autoreleasepool {
        assert(probe_entitlements_valid(@{@"com.apple.security.app-sandbox":@YES},YES));
        assert(!probe_entitlements_valid(@{@"com.apple.security.app-sandbox":@1},YES));
        assert(!probe_entitlements_valid(@{@"com.apple.security.app-sandbox":@"true"},YES));
        assert(!probe_entitlements_valid(@{@"com.apple.security.app-sandbox":@NO},YES));
        assert(!probe_entitlements_valid(@{@"com.apple.security.app-sandbox":@YES,@"extra":@YES},YES));
        assert(probe_entitlements_valid(@{},NO));
        assert(!probe_entitlements_valid(@{@"extra":@YES},NO));
        assert(probe_parse(probe_message("ping"),"ping"));
        assert(!probe_parse(probe_message("pong"),"ping"));
        xpc_object_t v=probe_message("ping"); xpc_dictionary_set_bool(v,"protocol",true); assert(!probe_parse(v,"ping"));
        v=probe_message("ping"); xpc_dictionary_set_string(v,"unknown","x"); assert(!probe_parse(v,"ping"));
        v=probe_message("ping"); xpc_dictionary_set_value(v,"payload",xpc_dictionary_create(NULL,NULL,0)); assert(!probe_parse(v,"ping"));
        char data[257]={0}; v=probe_message("ping"); xpc_dictionary_set_data(v,"payload",data,257); assert(!probe_parse(v,"ping"));
        xpc_dictionary_set_data(v,"payload",data,256); assert(probe_parse(v,"ping"));
        v=probe_message("ping"); xpc_dictionary_set_string(v,"requestId","public-request-0123456789abcdeg"); assert(!probe_parse(v,"ping"));
        ProbeOwner *a=[[ProbeOwner alloc] initWithDeadline:10], *b=[[ProbeOwner alloc] initWithDeadline:10];
        assert(![a acceptFrom:b now:1]); assert([a acceptFrom:a now:1]); assert(![a acceptFrom:a now:2]);
        [b retire]; assert(![b acceptFrom:b now:1]);
        ProbeOwner *c=[[ProbeOwner alloc] initWithDeadline:10]; assert(![c acceptFrom:c now:10]);
        ProbeOwner *newOwner=[[ProbeOwner alloc] initWithDeadline:20]; assert(![newOwner acceptFrom:c now:11]); assert([newOwner acceptFrom:newOwner now:11]);
        puts("local: 8 protocol cases + 7 owner + 7 entitlement assertions passed (no connections)");
    }
}
