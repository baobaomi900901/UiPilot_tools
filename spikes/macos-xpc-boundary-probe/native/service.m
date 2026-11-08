#import "preflight.h"
#import "adapter.h"
int main(void) {
    @autoreleasepool {
        NSURL *host=NSBundle.mainBundle.bundleURL;
        for (unsigned i=0;i<3;i++) host=host.URLByDeletingLastPathComponent;
        if (!probe_preflight(host,YES)) return 3;
        xpc_main(probe_accept_peer);
    }
}
