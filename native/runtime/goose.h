/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_GOOSE_H
#define TOOLKIT_GOOSE_H
#include "cJSON.h"
/** Observe a bounded subscription and preserve invalid frames and TTL expiry in its events. */
int Goose_subscribe(const cJSON* request);
#endif
