/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_RUNTIME_H
#define TOOLKIT_RUNTIME_H
#include <stdbool.h>
#include <stdint.h>
#include "cJSON.h"
/** Execute a versioned local request. Network is opened only by explicit live actions. */
int Runtime_execute(const cJSON* request);
bool Runtime_duration(const cJSON* request, uint64_t* duration);
bool Runtime_mac(const char* text, uint8_t* bytes);
#endif
