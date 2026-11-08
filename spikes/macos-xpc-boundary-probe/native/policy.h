#pragma once
#include <stdbool.h>
typedef struct {
    void *context;
    int (*requirement)(void *);
    void (*retire)(void *);
    void (*cancel)(void *);
    void (*activate)(void *, bool cleanup);
} ProbePolicy;
// Caller installs a safe handler first. A borrowed rejected peer is not retained.
bool probe_configure(ProbePolicy policy, bool owned);
