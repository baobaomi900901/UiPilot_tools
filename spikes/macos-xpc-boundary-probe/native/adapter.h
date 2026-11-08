#pragma once
#import "protocol.h"
// Must only be called by the separately authorized --run / service paths.
void probe_run_host(void);
void probe_accept_peer(xpc_connection_t peer);
