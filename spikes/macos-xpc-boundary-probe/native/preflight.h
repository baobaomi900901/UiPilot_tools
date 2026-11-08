#pragma once
#import <Foundation/Foundation.h>
// Local signature inspection only. No XPC connections, GUI, or trust network.
BOOL probe_entitlements_valid(id value, BOOL service);
BOOL probe_preflight(NSURL *hostBundle, BOOL runningService);
