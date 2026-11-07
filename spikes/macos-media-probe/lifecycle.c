#include "lifecycle.h"
void probe_retire(ProbeLifecycle *s) { s->phase = PROBE_RETIRED; s->current.instance = 0; }
ProbeTicket probe_begin(ProbeLifecycle *s, uintptr_t instance, bool wait) {
    probe_retire(s);
    if (!instance || s->exhausted || s->current.generation == UINT64_MAX) {
        if (s->current.generation == UINT64_MAX) s->exhausted = true;
        return (ProbeTicket){0};
    }
    s->current.generation++;
    s->current.instance = instance;
    s->phase = wait ? PROBE_WAITING : PROBE_READY;
    return s->current;
}
bool probe_current(const ProbeLifecycle *s, ProbeTicket t) {
    return !s->exhausted && s->phase != PROBE_RETIRED && t.generation && t.instance &&
        s->current.generation == t.generation && s->current.instance == t.instance;
}
bool probe_suspend_completed(ProbeLifecycle *s, ProbeTicket t) {
    if (!probe_current(s, t) || s->phase != PROBE_WAITING) return false;
    s->phase = PROBE_READY;
    return true;
}
bool probe_admit_load(ProbeLifecycle *s, ProbeTicket t) {
    if (!probe_current(s, t) || s->phase != PROBE_READY) return false;
    s->phase = PROBE_LOADED;
    return true;
}
bool probe_accept_message(const ProbeLifecycle *s, ProbeTicket t, bool main_frame, bool nonce_matches) {
    return probe_current(s, t) && s->phase == PROBE_LOADED && main_frame && nonce_matches;
}
