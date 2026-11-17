/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_VALUES_H
#define TOOLKIT_VALUES_H
#include "cJSON.h"
#include "iec61850_model.h"
#include "mms_value.h"
/** Clone and type-check a leaf value. Caller owns the returned MmsValue. */
MmsValue* Values_prepare(DataAttribute* attribute, const cJSON* value);
/** Convert a scalar using a remotely queried MMS specification; never infer type from current data. */
MmsValue* Values_from_spec(const MmsVariableSpecification* spec, const cJSON* value);
/** Validate and clone a received scalar for a specific model attribute. Caller owns the clone. */
MmsValue* Values_prepare_received(DataAttribute* attribute, const MmsValue* value);
/** Create a typed JSON representation; caller owns it. Traversal uses an explicit stack. */
cJSON* Values_json(const MmsValue* value);
#endif
