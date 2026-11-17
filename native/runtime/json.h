/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_JSON_H
#define TOOLKIT_JSON_H
#include <stdbool.h>
#include <stdint.h>
#include "cJSON.h"
/** Establish a dedicated protocol output; library stdout is redirected to stderr. */
bool Json_initialize(void);
void Json_close(void);
/** Echo the negotiated local protocol version; version 1 remains supported for older CLIs. */
void Json_set_protocol_version(uint32_t version);
/** Own the returned JSON tree; inputs are untrusted UTF-8 request files. */
cJSON* Json_read_file(const char* path);
const char* Json_string(const cJSON* object, const char* name);
bool Json_u32(const cJSON* object, const char* name, uint32_t* value);
bool Json_number(const cJSON* value, double min, double max);
/** Consumes data. Serialization is synchronized for native library callbacks. */
void Json_emit(const char* event, cJSON* data);
void Json_error(const char* code, const char* message);
void* Json_allocate(size_t size);
/** Numeric protocol fields use exact values in the IEEE-754 safe integer range. */
void Json_add_u64(cJSON* object, const char* key, uint64_t value);
#endif
