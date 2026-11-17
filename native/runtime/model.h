/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_MODEL_H
#define TOOLKIT_MODEL_H
#include <stdbool.h>
#include <stdint.h>
#include "cJSON.h"
#include "iec61850_server.h"
typedef struct ModelSession ModelSession;
/** Owns model, server, selected controls and prevalidated update values. No network on prepare. */
ModelSession* Model_prepare(const cJSON* request);
bool Model_start(ModelSession* session);
void Model_apply_due(ModelSession* session, uint64_t elapsed_ms);
bool Model_healthy(ModelSession* session);
void Model_destroy(ModelSession* session);
#endif
