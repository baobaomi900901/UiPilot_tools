#import "protocol.h"
#include <string.h>
bool probe_parse(xpc_object_t v, const char *kind) {
    if (!v || xpc_get_type(v)!=XPC_TYPE_DICTIONARY || xpc_dictionary_get_count(v)!=5) return false;
    const char *keys[]={"protocol","kind","requestId","sequence","payload"};
    xpc_type_t types[]={XPC_TYPE_UINT64,XPC_TYPE_STRING,XPC_TYPE_STRING,XPC_TYPE_UINT64,XPC_TYPE_DATA};
    for (unsigned i=0;i<5;i++) {
        xpc_object_t field=xpc_dictionary_get_value(v,keys[i]);
        if (!field || xpc_get_type(field)!=types[i]) return false;
    }
    const char *request=xpc_dictionary_get_string(v,"requestId");
    const char *wireKind=xpc_dictionary_get_string(v,"kind");
    if (xpc_string_get_length(xpc_dictionary_get_value(v,"kind"))!=strlen(kind) || strcmp(wireKind,kind)) return false;
    if (xpc_string_get_length(xpc_dictionary_get_value(v,"requestId"))!=31 || strncmp(request,"public-request-",15)) return false;
    for (unsigned i=15;i<31;i++) if (!((request[i]>='0'&&request[i]<='9') || (request[i]>='a'&&request[i]<='f'))) return false;
    return xpc_dictionary_get_uint64(v,"protocol")==1 && xpc_dictionary_get_uint64(v,"sequence")==1 &&
           xpc_data_get_length(xpc_dictionary_get_value(v,"payload"))<=256;
}
void probe_fill(xpc_object_t v, const char *kind) {
    xpc_dictionary_set_uint64(v,"protocol",1);
    xpc_dictionary_set_string(v,"kind",kind);
    xpc_dictionary_set_string(v,"requestId",PROBE_REQUEST);
    xpc_dictionary_set_uint64(v,"sequence",1);
    xpc_dictionary_set_data(v,"payload",PROBE_PAYLOAD,strlen(PROBE_PAYLOAD));
}
xpc_object_t probe_message(const char *kind) {
    xpc_object_t v=xpc_dictionary_create(NULL,NULL,0); probe_fill(v,kind); return v;
}
@implementation ProbeOwner
- (instancetype)initWithDeadline:(double)deadline {
    if ((self=[super init])) { _active=YES; _deadline=deadline; } return self;
}
- (BOOL)acceptFrom:(ProbeOwner *)source now:(double)now {
    if (source!=self || !_active || _completed || now>=_deadline) return NO;
    _completed=YES; _active=NO; return YES;
}
- (void)retire { _active=NO; }
@end
