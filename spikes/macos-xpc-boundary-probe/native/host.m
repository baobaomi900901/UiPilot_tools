#import "preflight.h"
#import "adapter.h"
int main(int argc,const char *argv[]) {
    @autoreleasepool {
        BOOL run=argc==2 && !strcmp(argv[1],"--run");
        if (argc>2 || (argc==2 && !run && strcmp(argv[1],"--preflight"))) return 2;
        BOOL eligible=probe_preflight(NSBundle.mainBundle.bundleURL,NO);
        if (!run || !eligible) {
            puts(eligible?"{\"status\":\"eligibleNotConnected\",\"connectionsCreated\":0}":
                          "{\"status\":\"blockedSigningIdentity\",\"connectionsCreated\":0}");
            return eligible?0:3;
        }
        probe_run_host();
    }
}
