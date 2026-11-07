#ifndef UIPILOT_MEDIA_LIFECYCLE_H
#define UIPILOT_MEDIA_LIFECYCLE_H
#include <stdbool.h>
#include <stdint.h>
typedef enum { PROBE_RETIRED, PROBE_WAITING, PROBE_READY, PROBE_LOADED } ProbePhase;
typedef struct { uint64_t generation; uintptr_t instance; } ProbeTicket;
typedef struct { ProbeTicket current; ProbePhase phase; bool exhausted; } ProbeLifecycle;
ProbeTicket probe_begin(ProbeLifecycle *, uintptr_t instance, bool wait_for_suspend);
bool probe_current(const ProbeLifecycle *, ProbeTicket);
bool probe_suspend_completed(ProbeLifecycle *, ProbeTicket);
bool probe_admit_load(ProbeLifecycle *, ProbeTicket);
bool probe_accept_message(const ProbeLifecycle *, ProbeTicket, bool main_frame, bool nonce_matches);
void probe_retire(ProbeLifecycle *);
#endif
