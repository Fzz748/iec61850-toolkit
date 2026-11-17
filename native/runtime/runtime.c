/* SPDX-License-Identifier: Apache-2.0 */
#include "runtime.h"
#include <math.h>
#include <stdio.h>
#include <string.h>
#include "hal_thread.h"
#include "hal_time.h"
#include "iec61850_common.h"
#include "goose.h"
#include "json.h"
#include "mms.h"
#include "model.h"
#include "platform.h"

bool Runtime_duration(const cJSON* request, uint64_t* duration)
{
    const cJSON* value = cJSON_GetObjectItemCaseSensitive(request,"duration_ms");
    if (!Json_number(value,1,4294967295000.0) || floor(value->valuedouble) != value->valuedouble)
        return false;
    *duration = (uint64_t) value->valuedouble;
    return true;
}

static int hex_digit(char digit)
{
    if (digit >= '0' && digit <= '9') return digit - '0';
    if (digit >= 'a' && digit <= 'f') return digit - 'a' + 10;
    if (digit >= 'A' && digit <= 'F') return digit - 'A' + 10;
    return -1;
}

bool Runtime_mac(const char* text, uint8_t* bytes)
{
    if (!text || strlen(text) != 17) return false;
    for (int i = 0; i < 6; i++) {
        int high = hex_digit(text[3*i]);
        int low = hex_digit(text[3*i+1]);
        if (high < 0 || low < 0) return false;
        if (i < 5 && text[3*i+2] != ':' && text[3*i+2] != '-') return false;
        bytes[i] = (uint8_t) (high * 16 + low);
    }
    const uint8_t broadcast[6] = {255,255,255,255,255,255};
    return (bytes[0] & 1) != 0 && memcmp(bytes,broadcast,6) != 0;
}

static int capabilities(void)
{
    cJSON* interfaces = Platform_interfaces();
    if (!interfaces) { Json_error("INTERFACES","cannot enumerate Ethernet interfaces"); return 2; }
    cJSON* data = cJSON_CreateObject();
    cJSON_AddStringToObject(data,"backend","libiec61850");
    cJSON_AddStringToObject(data,"library_version",LibIEC61850_getVersionString());
    cJSON_AddStringToObject(data,"tested_source_version","v1.6.2.1");
    cJSON_AddStringToObject(data,"integration_patch","enable-all GoCB success notifications; exclusive Windows MMS bind");
    cJSON_AddBoolToObject(data,"mms",true);
    cJSON_AddBoolToObject(data,"mms_write",true);
    cJSON_AddBoolToObject(data,"goose",true);
    cJSON_AddItemToObject(data,"interfaces",interfaces);
    Json_emit("result",data);
    return 0;
}

static cJSON* simulate(ModelSession* session, uint64_t duration)
{
    if (!Model_start(session)) return NULL;
    Json_emit("ready",cJSON_CreateObject());
    uint64_t start = Hal_getMonotonicTimeInMs();
    bool cancelled = false;
    while (Hal_getMonotonicTimeInMs() - start < duration) {
        if (Platform_stop_requested()) { cancelled = true; break; }
        if (!Model_healthy(session)) { Json_error("SESSION_FAILED","MMS listener or GOOSE publisher stopped"); return NULL; }
        Model_apply_due(session,Hal_getMonotonicTimeInMs()-start);
        Thread_sleep(1);
    }
    cJSON* data = cJSON_CreateObject();
    cJSON_AddStringToObject(data,"stop_reason",cancelled ? "parent_stop" : "duration");
    Json_add_u64(data,"elapsed_ms",Hal_getMonotonicTimeInMs()-start);
    return data;
}

int Runtime_execute(const cJSON* request)
{
    uint32_t version;
    const char* action = Json_string(request,"action");
    if (!Json_u32(request,"protocol_version",&version) || (version != 1 && version != 2) || !action) {
        Json_error("PROTOCOL","requires protocol_version=1 or 2 and action"); return 2;
    }
    Json_set_protocol_version(version);
    if (strcmp(action,"capabilities") == 0) return capabilities();
    if (strcmp(action,"browse") == 0 || strcmp(action,"read") == 0 || strcmp(action,"write") == 0) return Mms_execute(request);
    if (strcmp(action,"subscribe") == 0) return Goose_subscribe(request);
    bool check = strcmp(action,"check") == 0;
    uint64_t duration = 0;
    if (!check && (strcmp(action,"simulate") != 0 || !Runtime_duration(request,&duration))) {
        Json_error("PROTOCOL","unknown action or invalid duration_ms"); return 2;
    }
    ModelSession* session = Model_prepare(request);
    if (!session) return 2;
    cJSON* result;
    if (check) {
        result = cJSON_CreateObject();
        cJSON_AddBoolToObject(result,"network_started",false);
    } else result = simulate(session,duration);
    Model_destroy(session);
    /* Join network callbacks before the terminal event; writes cannot follow result. */
    if (!result) return 2;
    Json_emit("result",result);
    return 0;
}
