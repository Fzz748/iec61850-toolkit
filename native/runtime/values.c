/* SPDX-License-Identifier: Apache-2.0 */
#include "values.h"
#include <errno.h>
#include <float.h>
#include <inttypes.h>
#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "json.h"

static bool integer_value(const cJSON* input, double min, double max)
{
    return Json_number(input, min, max) && input->valuedouble == floor(input->valuedouble);
}

static bool set_signed(MmsValue* output, const cJSON* input, DataAttributeType type)
{
    if (type != IEC61850_INT8 && type != IEC61850_INT16 && type != IEC61850_INT32
        && type != IEC61850_INT64 && type != IEC61850_ENUMERATED) return false;
    if (type == IEC61850_INT64) {
        if (!cJSON_IsString(input) || !input->valuestring[0]) return false;
        errno = 0;
        char* end;
        int64_t value = strtoll(input->valuestring, &end, 10);
        if (errno || *end) return false;
        MmsValue_setInt64(output, value);
        return true;
    }
    double min = INT32_MIN, max = INT32_MAX;
    if (type == IEC61850_INT8) { min = INT8_MIN; max = INT8_MAX; }
    if (type == IEC61850_INT16) { min = INT16_MIN; max = INT16_MAX; }
    if (!integer_value(input, min, max)) return false;
    MmsValue_setInt32(output, (int32_t) input->valuedouble);
    return true;
}

static bool set_unsigned(MmsValue* output, const cJSON* input, DataAttributeType type)
{
    double max = UINT32_MAX;
    if (type == IEC61850_INT8U) max = UINT8_MAX;
    if (type == IEC61850_INT16U) max = UINT16_MAX;
    if (type == IEC61850_INT24U) max = 0xffffff;
    if (!integer_value(input, 0, max)) return false;
    MmsValue_setUint32(output, (uint32_t) input->valuedouble);
    return true;
}

static bool set_string(MmsValue* output, const cJSON* input, DataAttributeType type)
{
    static const size_t lengths[] = {32, 64, 65, 129, 255, 255};
    if (type < IEC61850_VISIBLE_STRING_32 || type > IEC61850_UNICODE_STRING_255)
        return false;
    if (!cJSON_IsString(input) || strlen(input->valuestring) > lengths[type - IEC61850_VISIBLE_STRING_32])
        return false;
    if (type == IEC61850_UNICODE_STRING_255) MmsValue_setMmsString(output, input->valuestring);
    else MmsValue_setVisibleString(output, input->valuestring);
    return true;
}

static bool set_bits(MmsValue* output, const cJSON* input)
{
    int bits = MmsValue_getBitStringSize(output);
    if (bits < 1 || bits > 32) return false;
    double max = bits == 32 ? UINT32_MAX : ((uint32_t) 1 << bits) - 1;
    if (!integer_value(input, 0, max)) return false;
    MmsValue_setBitStringFromInteger(output, (uint32_t) input->valuedouble);
    return true;
}

static bool set_scalar(MmsValue* output, const cJSON* input, DataAttributeType type)
{
    switch (MmsValue_getType(output)) {
    case MMS_BOOLEAN:
        if (!cJSON_IsBool(input)) return false;
        MmsValue_setBoolean(output, cJSON_IsTrue(input)); return true;
    case MMS_INTEGER: return set_signed(output, input, type);
    case MMS_UNSIGNED: return set_unsigned(output, input, type);
    case MMS_FLOAT:
        if (!Json_number(input, -DBL_MAX, DBL_MAX)) return false;
        if (type == IEC61850_FLOAT32 && fabs(input->valuedouble) > FLT_MAX) return false;
        if (type == IEC61850_FLOAT32) MmsValue_setFloat(output, (float) input->valuedouble);
        else MmsValue_setDouble(output, input->valuedouble);
        return true;
    case MMS_VISIBLE_STRING: case MMS_STRING: return set_string(output, input, type);
    case MMS_BIT_STRING: return set_bits(output, input);
    case MMS_UTC_TIME:
        if (!integer_value(input, 0, 4294967295999.0)) return false;
        MmsValue_setUtcTimeMs(output, (uint64_t) input->valuedouble); return true;
    default: return false;
    }
}

MmsValue* Values_prepare(DataAttribute* attribute, const cJSON* value)
{
    if (!attribute->mmsValue || attribute->firstChild || attribute->elementCount > 0)
        return NULL;
    MmsValue* prepared = MmsValue_clone(attribute->mmsValue);
    if (!prepared) return NULL;
    if (set_scalar(prepared, value, attribute->type)) return prepared;
    MmsValue_delete(prepared);
    return NULL;
}

static DataAttributeType spec_type(const MmsVariableSpecification* spec)
{
    switch (spec->type) {
    case MMS_BOOLEAN: return IEC61850_BOOLEAN;
    case MMS_UTC_TIME: return IEC61850_TIMESTAMP;
    case MMS_INTEGER:
        if (spec->typeSpec.integer < 1 || spec->typeSpec.integer > 64) break;
        return spec->typeSpec.integer <= 32 ? IEC61850_INT32 : IEC61850_INT64;
    case MMS_UNSIGNED:
        if (spec->typeSpec.unsignedInteger >= 1 && spec->typeSpec.unsignedInteger <= 32)
            return IEC61850_INT32U;
        break;
    case MMS_FLOAT:
        if (spec->typeSpec.floatingpoint.formatWidth == 32 && spec->typeSpec.floatingpoint.exponentWidth == 8)
            return IEC61850_FLOAT32;
        if (spec->typeSpec.floatingpoint.formatWidth == 64 && spec->typeSpec.floatingpoint.exponentWidth == 11)
            return IEC61850_FLOAT64;
        break;
    case MMS_BIT_STRING:
        if (spec->typeSpec.bitString >= -32 && spec->typeSpec.bitString <= 32 && spec->typeSpec.bitString != 0)
            return IEC61850_GENERIC_BITSTRING;
        break;
    default: break;
    }
    return IEC61850_UNKNOWN_TYPE;
}

static MmsValue* string_from_spec(const MmsVariableSpecification* spec, const cJSON* value)
{
    if (!cJSON_IsString(value)) return NULL;
    int64_t size = spec->type == MMS_STRING ? spec->typeSpec.mmsString : spec->typeSpec.visibleString;
    if (size < 0) size = -size;
    if ((uint64_t) strlen(value->valuestring) > (uint64_t) size) return NULL;
    /* Allocate the supplied value, not a potentially enormous remote maximum. */
    return spec->type == MMS_STRING ? MmsValue_newMmsString(value->valuestring)
        : MmsValue_newVisibleString(value->valuestring);
}

static bool fits_spec(const MmsVariableSpecification* spec, const MmsValue* value)
{
    if (spec->type == MMS_UNSIGNED)
        return (double) MmsValue_toUint32(value) < ldexp(1.0,spec->typeSpec.unsignedInteger);
    if (spec->type == MMS_INTEGER && spec->typeSpec.integer < 64) {
        int64_t limit = INT64_C(1) << (spec->typeSpec.integer - 1);
        int64_t number = MmsValue_toInt64(value);
        return number >= -limit && number < limit;
    }
    return true;
}

MmsValue* Values_from_spec(const MmsVariableSpecification* spec, const cJSON* value)
{
    if (spec->type == MMS_VISIBLE_STRING || spec->type == MMS_STRING)
        return string_from_spec(spec,value);
    DataAttributeType type = spec_type(spec);
    if (type == IEC61850_UNKNOWN_TYPE) return NULL;
    MmsValue* result;
    /* Use full supported integer storage, then check the remote width without truncation. */
    if (spec->type == MMS_INTEGER) result = MmsValue_newIntegerFromInt64(0);
    else if (spec->type == MMS_UNSIGNED) result = MmsValue_newUnsignedFromUint32(0);
    else result = MmsValue_newDefaultValue(spec);
    if (!result) return NULL;
    if (set_scalar(result,value,type) && fits_spec(spec,result)) return result;
    MmsValue_delete(result);
    return NULL;
}

MmsValue* Values_prepare_received(DataAttribute* attribute, const MmsValue* value)
{
    if (!attribute->mmsValue || !MmsValue_equalTypes(attribute->mmsValue,value)) return NULL;
    cJSON* encoded = Values_json(value);
    if (!encoded) return NULL;
    cJSON* scalar = cJSON_GetObjectItemCaseSensitive(encoded,"value");
    cJSON* numeric = NULL;
    if (MmsValue_getType(value) == MMS_INTEGER && attribute->type != IEC61850_INT64)
        numeric = cJSON_CreateNumber((double) MmsValue_toInt64(value));
    if (MmsValue_getType(value) == MMS_BIT_STRING && MmsValue_getBitStringSize(value) <= 32)
        numeric = cJSON_CreateNumber(MmsValue_getBitStringAsInteger(value));
    MmsValue* result = Values_prepare(attribute,numeric ? numeric : scalar);
    cJSON_Delete(numeric); cJSON_Delete(encoded);
    return result;
}

static cJSON* octets_json(const MmsValue* value)
{
    int length = MmsValue_getOctetStringSize(value);
    const uint8_t* bytes = MmsValue_getOctetStringBuffer((MmsValue*) value);
    char* hex = Json_allocate((size_t) length * 2 + 1);
    const char digits[] = "0123456789abcdef";
    for (int i = 0; i < length; i++) {
        hex[2*i] = digits[bytes[i] >> 4];
        hex[2*i + 1] = digits[bytes[i] & 15];
    }
    hex[2*length] = 0;
    cJSON* result = cJSON_CreateString(hex);
    free(hex);
    return result;
}

static cJSON* bits_json(const MmsValue* value)
{
    int size = MmsValue_getBitStringSize(value);
    char* bits = Json_allocate((size_t) size + 1);
    for (int i = 0; i < size; i++) bits[i] = MmsValue_getBitStringBit(value, i) ? '1' : '0';
    bits[size] = 0;
    cJSON* result = cJSON_CreateString(bits);
    free(bits);
    return result;
}

static cJSON* integer_json(const MmsValue* value)
{
    /* Refuse values that the public fixed-width conversion functions would truncate. */
    int size = MmsValue_encodeMmsData((MmsValue*) value,NULL,0,false);
    if (MmsValue_getType(value) == MMS_INTEGER) {
        if (size < 3 || size > 10) return NULL;
        char integer[32];
        snprintf(integer,sizeof(integer),"%" PRId64,MmsValue_toInt64(value));
        return cJSON_CreateString(integer);
    }
    if (size < 3 || size > 7) return NULL;
    uint8_t encoded[7];
    if (MmsValue_encodeMmsData((MmsValue*) value,encoded,0,true) != size) return NULL;
    if (size == 7 && encoded[2] != 0) return NULL;
    return cJSON_CreateNumber(MmsValue_toUint32(value));
}

static cJSON* scalar_json(const MmsValue* value)
{
    switch (MmsValue_getType(value)) {
    case MMS_BOOLEAN: return cJSON_CreateBool(MmsValue_getBoolean(value));
    case MMS_INTEGER: case MMS_UNSIGNED: return integer_json(value);
    case MMS_FLOAT:
        if (isnan(MmsValue_toDouble(value))) return cJSON_CreateString("NaN");
        if (isinf(MmsValue_toDouble(value)))
            return cJSON_CreateString(MmsValue_toDouble(value) < 0 ? "-Infinity" : "Infinity");
        return cJSON_CreateNumber(MmsValue_toDouble(value));
    case MMS_VISIBLE_STRING: case MMS_STRING:
        /* Upstream accessors lack const qualifiers but do not mutate the value. */
        return cJSON_CreateString(MmsValue_toString((MmsValue*) value));
    case MMS_UTC_TIME: return cJSON_CreateNumber((double) MmsValue_getUtcTimeInMs(value));
    case MMS_BINARY_TIME: return cJSON_CreateNumber((double) MmsValue_getBinaryTimeAsUtcMs(value));
    case MMS_OCTET_STRING: return octets_json(value);
    case MMS_BIT_STRING: return bits_json(value);
    case MMS_DATA_ACCESS_ERROR: return cJSON_CreateNumber(MmsValue_getDataAccessError(value));
    default: return NULL;
    }
}

typedef struct ValueWork {
    const MmsValue* value;
    cJSON* target;
    struct ValueWork* next;
} ValueWork;

static void push(ValueWork** stack, const MmsValue* value, cJSON* target)
{
    ValueWork* work = Json_allocate(sizeof(*work));
    work->value = value; work->target = target; work->next = *stack; *stack = work;
}

static bool expand(ValueWork* work, ValueWork** stack)
{
    const MmsValue* value = work->value;
    cJSON_AddStringToObject(work->target, "type", MmsValue_getTypeString((MmsValue*) value));
    MmsType type = MmsValue_getType(value);
    if (type != MMS_ARRAY && type != MMS_STRUCTURE) {
        cJSON* scalar = scalar_json(value);
        if (!scalar) return false;
        cJSON_AddItemToObject(work->target, "value", scalar);
        return true;
    }
    cJSON* children = cJSON_AddArrayToObject(work->target, "value");
    for (uint32_t i = 0; i < MmsValue_getArraySize(value); i++) {
        cJSON* item = cJSON_CreateObject();
        cJSON_AddItemToArray(children, item);
        const MmsValue* child = MmsValue_getElement(value, i);
        if (!child) return false;
        push(stack, child, item);
    }
    return true;
}

cJSON* Values_json(const MmsValue* value)
{
    if (!value) return NULL;
    cJSON* result = cJSON_CreateObject();
    ValueWork* stack = NULL;
    push(&stack, value, result);
    bool valid = true;
    while (stack) {
        ValueWork* work = stack; stack = work->next;
        if (valid) valid = expand(work, &stack);
        free(work);
    }
    if (!valid) { cJSON_Delete(result); return NULL; }
    return result;
}
