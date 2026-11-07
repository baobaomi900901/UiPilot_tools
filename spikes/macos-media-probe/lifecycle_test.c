#include "lifecycle.h"
#include <assert.h>
#include <stdio.h>
static void preparation(void) {
    ProbeLifecycle s = {0}; ProbeTicket t = probe_begin(&s, 10, true);
    assert(probe_current(&s, t)); assert(!probe_admit_load(&s, t));
    assert(!probe_accept_message(&s, t, true, true));
    assert(probe_suspend_completed(&s, t)); assert(!probe_suspend_completed(&s, t));
    assert(probe_admit_load(&s, t)); assert(!probe_admit_load(&s, t));
    assert(probe_accept_message(&s, t, true, true));
    assert(!probe_accept_message(&s, t, false, true));
    assert(!probe_accept_message(&s, t, true, false));
}
static void retirement(void) {
    ProbeLifecycle s = {0}; ProbeTicket old = probe_begin(&s, 10, true);
    probe_retire(&s); assert(!probe_suspend_completed(&s, old)); assert(!probe_admit_load(&s, old));
    ProbeTicket fresh = probe_begin(&s, 11, true);
    assert(!probe_suspend_completed(&s, old)); assert(probe_current(&s, fresh));
    ProbeTicket reused = probe_begin(&s, 11, false);
    assert(!probe_current(&s, fresh)); assert(probe_admit_load(&s, reused));
    assert(!probe_accept_message(&s, fresh, true, true));
    ProbeTicket forged = reused; forged.instance = 10;
    assert(!probe_accept_message(&s, forged, true, true));
    probe_retire(&s); assert(!probe_accept_message(&s, reused, true, true));
}
static void exhaustion(void) {
    ProbeLifecycle s = {0}; s.current.generation = UINT64_MAX - 1;
    ProbeTicket t = probe_begin(&s, 10, false); assert(t.generation == UINT64_MAX);
    assert(probe_admit_load(&s, t));
    assert(probe_begin(&s, 11, false).generation == 0); assert(s.exhausted);
    assert(!probe_current(&s, t)); assert(probe_begin(&s, 10, false).generation == 0);
    ProbeLifecycle empty = {0}; assert(probe_begin(&empty, 0, false).generation == 0);
}
int main(void) { preparation(); retirement(); exhaustion(); puts("3 lifecycle groups passed (admission, retirement/identity, exhaustion)"); }
