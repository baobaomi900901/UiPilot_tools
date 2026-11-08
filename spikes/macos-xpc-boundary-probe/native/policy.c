#include "policy.h"
bool probe_configure(ProbePolicy p, bool owned) {
    if (p.requirement(p.context) != 0) {
        p.retire(p.context);
        p.cancel(p.context);
        // The SDK forbids releasing an owned, inactive connection. Activate
        // only after cancellation to complete disposal, never to send payload.
        if (owned) p.activate(p.context, true);
        return false;
    }
    p.activate(p.context, false);
    return true;
}
