/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_PLATFORM_H
#define TOOLKIT_PLATFORM_H
#include <stdbool.h>
#include "cJSON.h"
/** Returns true on a stop byte or parent pipe closure. Does not block. */
bool Platform_stop_requested(void);
/** Enumerates interfaces without opening an Ethernet capture/injection handle. */
cJSON* Platform_interfaces(void);
/** Resolve an exact interface ID; caller owns the result. Rejects unknown IDs instead of selecting index 0. */
char* Platform_resolve_interface(const char* id);
#endif
