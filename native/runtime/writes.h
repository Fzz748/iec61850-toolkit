/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_WRITES_H
#define TOOLKIT_WRITES_H
#include "cJSON.h"
#include "iec61850_server.h"
typedef struct WritePolicy WritePolicy;
/** Install handlers for explicitly listed scalar attributes. No network I/O. */
WritePolicy* Writes_create(IedServer server, IedModel* model, const cJSON* mms);
/** Destroy only after stopping/destroying the server so no callback can use its bindings. */
void Writes_destroy(WritePolicy* policy);
#endif
