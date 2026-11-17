/* SPDX-License-Identifier: Apache-2.0 */
#include "values.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "json.h"

static void require(bool condition, const char* message)
{
    if (!condition) { fprintf(stderr,"value test failed: %s\n",message); exit(1); }
}

static void signed_values(void)
{
    MmsValue* value = MmsValue_newIntegerFromInt64(INT64_MIN);
    cJSON* output = Values_json(value);
    require(output != NULL,"INT64_MIN serialization");
    require(strcmp(Json_string(output,"value"),"-9223372036854775808") == 0,"signed precision");
    cJSON_Delete(output); MmsValue_delete(value);
    uint8_t wide[] = {0x85,9,1,0,0,0,0,0,0,0,0};
    value = MmsValue_decodeMmsData(wide,0,sizeof(wide),NULL);
    /* The pinned wire decoder rejects >64-bit signed data before conversion. */
    if (value) {
        require(Values_json(value) == NULL,"wide signed must not truncate");
        MmsValue_delete(value);
    }
}

static void unsigned_values(void)
{
    MmsValue* value = MmsValue_newUnsignedFromUint32(UINT32_MAX);
    cJSON* output = Values_json(value);
    require(output != NULL,"UINT32_MAX serialization");
    require(cJSON_GetObjectItemCaseSensitive(output,"value")->valuedouble == UINT32_MAX,"unsigned precision");
    cJSON_Delete(output); MmsValue_delete(value);
    uint8_t wide[] = {0x86,5,1,0,0,0,0};
    value = MmsValue_decodeMmsData(wide,0,sizeof(wide),NULL);
    if (value) {
        require(Values_json(value) == NULL,"wide unsigned must not truncate");
        MmsValue_delete(value);
    }
}

static void invalid_updates(void)
{
    DataAttribute attribute = {0};
    attribute.type = IEC61850_INT8;
    attribute.mmsValue = MmsValue_newIntegerFromInt8(0);
    cJSON* input = cJSON_CreateNumber(128);
    require(Values_prepare(&attribute,input) == NULL,"INT8 overflow");
    cJSON_Delete(input); MmsValue_delete(attribute.mmsValue);
    attribute.type = IEC61850_BOOLEAN;
    attribute.mmsValue = MmsValue_newBoolean(false);
    input = cJSON_CreateNumber(1);
    require(Values_prepare(&attribute,input) == NULL,"boolean type mismatch");
    cJSON_Delete(input); MmsValue_delete(attribute.mmsValue);
}

static void remote_write_types(void)
{
    MmsVariableSpecification spec = {0};
    spec.type = MMS_INTEGER; spec.typeSpec.integer = 8;
    cJSON* input = cJSON_CreateNumber(128);
    require(Values_from_spec(&spec,input) == NULL,"remote INT8 overflow is not truncated");
    cJSON_Delete(input); input = cJSON_CreateNumber(-128);
    MmsValue* value = Values_from_spec(&spec,input);
    require(value && MmsValue_toInt64(value) == -128,"remote INT8 minimum");
    MmsValue_delete(value); cJSON_Delete(input);
    spec.typeSpec.integer = 64;
    input = cJSON_CreateString("9223372036854775807");
    value = Values_from_spec(&spec,input);
    require(value && MmsValue_toInt64(value) == INT64_MAX,"remote INT64 precision");
    MmsValue_delete(value); cJSON_Delete(input);
    spec.type = MMS_UNSIGNED; spec.typeSpec.unsignedInteger = 8;
    input = cJSON_CreateNumber(256);
    require(Values_from_spec(&spec,input) == NULL,"remote UINT8 overflow");
    cJSON_Delete(input);
    spec.type = MMS_VISIBLE_STRING; spec.typeSpec.visibleString = -3;
    input = cJSON_CreateString("four");
    require(Values_from_spec(&spec,input) == NULL,"remote string maximum");
    cJSON_Delete(input);
}

static void received_write_range(void)
{
    DataAttribute attribute = {0};
    attribute.type = IEC61850_INT8;
    attribute.mmsValue = MmsValue_newIntegerFromInt8(0);
    MmsValue* incoming = MmsValue_newIntegerFromInt32(256);
    require(Values_prepare_received(&attribute,incoming) == NULL,"server rejects out-of-range wire value");
    MmsValue_setInt32(incoming,127);
    MmsValue* prepared = Values_prepare_received(&attribute,incoming);
    require(prepared && MmsValue_toInt32(prepared) == 127,"server accepts valid wire value");
    MmsValue_delete(prepared); MmsValue_delete(incoming); MmsValue_delete(attribute.mmsValue);
}

int main(void)
{
    require(Json_initialize(),"JSON setup");
    signed_values(); unsigned_values(); invalid_updates(); remote_write_types(); received_write_range();
    Json_close();
    return 0;
}
