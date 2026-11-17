/* SPDX-License-Identifier: Apache-2.0 */
#include "goose.h"
#include <stdlib.h>
#include <string.h>
#include "goose_receiver.h"
#include "goose_subscriber.h"
#include "hal_thread.h"
#include "hal_time.h"
#include "json.h"
#include "platform.h"
#include "runtime.h"
#include "values.h"

typedef struct {
    const char* dataset;
    uint32_t conf_rev;
    Semaphore lock;
    uint64_t frames;
    uint64_t invalid_frames;
    uint64_t last_frame;
    uint32_t ttl;
    uint64_t timeouts;
    bool expired;
    bool failed;
    char* interface;
} Subscription;

static void receive(GooseSubscriber subscriber, void* parameter)
{
    Subscription* state = parameter;
    cJSON* values = Values_json(GooseSubscriber_getDataSetValues(subscriber));
    const char* dataset = GooseSubscriber_getDataSet(subscriber);
    bool valid = GooseSubscriber_isValid(subscriber) && dataset && strcmp(dataset,state->dataset) == 0
        && GooseSubscriber_getConfRev(subscriber) == state->conf_rev;
    Semaphore_wait(state->lock);
    state->frames++;
    if (!valid) state->invalid_frames++;
    uint64_t received_ms = Hal_getMonotonicTimeInMs();
    state->last_frame = received_ms;
    state->ttl = GooseSubscriber_getTimeAllowedToLive(subscriber);
    state->expired = false;
    if (!values) state->failed = true;
    Semaphore_post(state->lock);
    if (!values) { Json_error("GOOSE_VALUE","cannot serialize received data"); return; }
    cJSON* event = cJSON_CreateObject();
    cJSON_AddBoolToObject(event,"valid",valid);
    Json_add_u64(event,"received_monotonic_ms",received_ms);
    cJSON_AddStringToObject(event,"gocb_ref",GooseSubscriber_getGoCbRef(subscriber));
    cJSON_AddStringToObject(event,"dataset",dataset ? dataset : "");
    cJSON_AddNumberToObject(event,"conf_rev",GooseSubscriber_getConfRev(subscriber));
    cJSON_AddNumberToObject(event,"st_num",GooseSubscriber_getStNum(subscriber));
    cJSON_AddNumberToObject(event,"sq_num",GooseSubscriber_getSqNum(subscriber));
    cJSON_AddNumberToObject(event,"ttl_ms",GooseSubscriber_getTimeAllowedToLive(subscriber));
    cJSON_AddNumberToObject(event,"parse_error",GooseSubscriber_getParseError(subscriber));
    cJSON_AddItemToObject(event,"values",values);
    Json_emit("goose",event);
}

static bool check_expiry(Subscription* state)
{
    Semaphore_wait(state->lock);
    bool expired = state->frames && !state->expired
        && Hal_getMonotonicTimeInMs() - state->last_frame >= state->ttl;
    if (expired) { state->expired = true; state->timeouts++; }
    bool failed = state->failed;
    Semaphore_post(state->lock);
    if (expired) Json_emit("goose_timeout",cJSON_CreateObject());
    return !failed;
}

static GooseReceiver create_receiver(const cJSON* config, Subscription* state)
{
    const char* interface = Json_string(config,"interface");
    const char* ref = Json_string(config,"gocb_ref");
    const char* mac = Json_string(config,"mac");
    uint32_t appid;
    uint8_t bytes[6];
    state->dataset = Json_string(config,"dataset");
    if (!interface || !ref || !mac || !state->dataset || !Runtime_mac(mac,bytes)
        || !Json_u32(config,"appid",&appid) || appid > 0x3fff || !Json_u32(config,"conf_rev",&state->conf_rev))
        return NULL;
    state->interface = Platform_resolve_interface(interface);
    if (!state->interface) return NULL;
    GooseReceiver receiver = GooseReceiver_create();
    GooseSubscriber subscriber = GooseSubscriber_create((char*) ref,NULL);
    if (!receiver || !subscriber) {
        if (receiver) GooseReceiver_destroy(receiver);
        if (subscriber) GooseSubscriber_destroy(subscriber);
        return NULL;
    }
    GooseSubscriber_setAppId(subscriber,(uint16_t) appid);
    GooseSubscriber_setDstMac(subscriber,bytes);
    GooseSubscriber_setListener(subscriber,receive,state);
    GooseReceiver_setInterfaceId(receiver,state->interface);
    GooseReceiver_addSubscriber(receiver,subscriber);
    return receiver;
}

static int observe(GooseReceiver receiver, Subscription* state, uint64_t duration)
{
    GooseReceiver_start(receiver);
    if (!GooseReceiver_isRunning(receiver)) { Json_error("GOOSE_START","cannot open selected interface"); return 2; }
    Json_emit("ready",cJSON_CreateObject());
    uint64_t start = Hal_getMonotonicTimeInMs();
    bool cancelled = false, healthy = true;
    while (Hal_getMonotonicTimeInMs() - start < duration) {
        if (Platform_stop_requested()) { cancelled = true; break; }
        if (!GooseReceiver_isRunning(receiver) || !check_expiry(state)) { healthy = false; break; }
        Thread_sleep(10);
    }
    GooseReceiver_stop(receiver);
    if (!healthy) { Json_error("GOOSE_RECEIVE","receiver stopped or event decoding failed"); return 2; }
    cJSON* result = cJSON_CreateObject();
    Json_add_u64(result,"frames",state->frames);
    Json_add_u64(result,"invalid_frames",state->invalid_frames);
    Json_add_u64(result,"timeouts",state->timeouts);
    cJSON_AddBoolToObject(result,"validation_failed",state->invalid_frames || state->timeouts);
    cJSON_AddBoolToObject(result,"observed",state->frames > 0);
    cJSON_AddStringToObject(result,"stop_reason",cancelled ? "parent_stop" : "duration");
    Json_emit("result",result);
    return 0;
}

int Goose_subscribe(const cJSON* request)
{
    uint64_t duration;
    if (!Runtime_duration(request,&duration)) { Json_error("DURATION","invalid duration_ms"); return 2; }
    Subscription state = {0};
    state.lock = Semaphore_create(1);
    if (!state.lock) { Json_error("GOOSE_CREATE","cannot create state lock"); return 2; }
    GooseReceiver receiver = create_receiver(cJSON_GetObjectItemCaseSensitive(request,"subscription"),&state);
    if (!receiver) { free(state.interface); Semaphore_destroy(state.lock); Json_error("GOOSE_CONFIG","invalid subscription or unknown interface ID"); return 2; }
    int result = observe(receiver,&state,duration);
    GooseReceiver_destroy(receiver);
    free(state.interface);
    Semaphore_destroy(state.lock);
    return result;
}
