/* SPDX-License-Identifier: Apache-2.0 */
#include "model.h"
#include <stdlib.h>
#include <string.h>
#include "hal_thread.h"
#include "iec61850_config_file_parser.h"
#include "mms_server.h"
#include "json.h"
#include "platform.h"
#include "runtime.h"
#include "values.h"
#include "writes.h"

typedef struct {
    DataAttribute* attribute;
    MmsValue* value;
    const char* reference;
} Update;

typedef struct {
    uint32_t at_ms;
    size_t count;
    Update* updates;
} Step;

struct ModelSession {
    const cJSON* request;
    IedModel* model;
    IedServer server;
    GSEControlBlock** original_controls;
    size_t original_count;
    MmsGooseControlBlock* active_controls;
    size_t selected_count;
    Semaphore control_lock;
    Step initial;
    Step* steps;
    size_t step_count;
    size_t next_step;
    bool mms_started;
    bool goose_started;
    char* interface;
    WritePolicy* write_policy;
};

static bool fail(const char* code, const char* message)
{
    Json_error(code, message);
    return false;
}

static const cJSON* control_selection(ModelSession* session, GSEControlBlock* cb)
{
    const cJSON* selected;
    const cJSON* controls = cJSON_GetObjectItemCaseSensitive(session->request, "controls");
    const char* ld = cb->parent->parent->name;
    cJSON_ArrayForEach(selected, controls) {
        const char* name = Json_string(selected, "name");
        const char* instance = Json_string(selected, "ld_inst");
        if (name && instance && strcmp(name, cb->name) == 0 && strcmp(instance, ld) == 0)
            return selected;
    }
    return NULL;
}

static bool configure_address(GSEControlBlock* cb, const cJSON* selection)
{
    const cJSON* address = cJSON_GetObjectItemCaseSensitive(selection, "address");
    uint32_t appid, vid, priority, min, max;
    const char* mac = Json_string(address, "mac");
    if (!mac || !Json_u32(address,"appid",&appid) || !Json_u32(address,"vlan_id",&vid)
        || !Json_u32(address,"vlan_priority",&priority) || !Json_u32(address,"min_time_ms",&min)
        || !Json_u32(address,"max_time_ms",&max)) return false;
    if (appid > 0x3fff || vid > 4094 || priority > 7 || !min || min > max || max > INT32_MAX / 3)
        return false;
    if (!cb->address) cb->address = Json_allocate(sizeof(*cb->address));
    if (!Runtime_mac(mac, cb->address->dstAddress)) return false;
    cb->address->appId = (uint16_t) appid;
    cb->address->vlanId = (uint16_t) vid;
    cb->address->vlanPriority = (uint8_t) priority;
    cb->minTime = (int) min; cb->maxTime = (int) max;
    return true;
}

static bool check_members(DataSet* dataset, const cJSON* selection)
{
    const cJSON* member;
    DataSetEntry* entry = dataset->fcdas;
    cJSON_ArrayForEach(member,cJSON_GetObjectItemCaseSensitive(selection,"members")) {
        const char* ld = Json_string(member,"ld_inst");
        const char* variable = Json_string(member,"variable");
        if (!entry || !ld || !variable || strcmp(entry->logicalDeviceName,ld)
            || strcmp(entry->variableName,variable) || entry->index != -1 || entry->componentName)
            return false;
        entry = entry->sibling;
    }
    return entry == NULL;
}

static bool check_dataset(ModelSession* session, GSEControlBlock* cb, const cJSON* selection)
{
    const char* name = Json_string(selection, "dataset");
    uint32_t expected;
    if (!name || !Json_u32(selection, "member_count", &expected)) return false;
    if (!cb->dataSetName || strcmp(name, cb->dataSetName)) return false;
    for (DataSet* ds = session->model->dataSets; ds; ds = ds->sibling) {
        const char* separator = strchr(ds->name, '$');
        if (!separator || strcmp(ds->logicalDeviceName, cb->parent->parent->name)) continue;
        if ((size_t) (separator - ds->name) != strlen(cb->parent->name)) continue;
        if (strncmp(ds->name, cb->parent->name, (size_t) (separator - ds->name))) continue;
        if (strcmp(separator + 1, name) == 0)
            return ds->elementCount == (int) expected && expected > 0 && check_members(ds,selection);
    }
    return false;
}

static bool select_controls(ModelSession* session)
{
    for (GSEControlBlock* cb = session->model->gseCBs; cb; cb = cb->sibling) session->original_count++;
    session->original_controls = Json_allocate((session->original_count + 1) * sizeof(GSEControlBlock*));
    size_t index = 0;
    for (GSEControlBlock* cb = session->model->gseCBs; cb; cb = cb->sibling)
        session->original_controls[index++] = cb;
    GSEControlBlock** tail = &session->model->gseCBs;
    *tail = NULL;
    for (size_t i = 0; i < session->original_count; i++) {
        GSEControlBlock* cb = session->original_controls[i];
        const cJSON* selection = control_selection(session, cb);
        if (!selection) continue;
        if (!configure_address(cb, selection) || !check_dataset(session, cb, selection))
            return fail("MODEL_GOOSE", "invalid address or converted DataSet differs from SCL selection");
        *tail = cb; cb->sibling = NULL; tail = &cb->sibling;
        session->selected_count++;
    }
    const cJSON* selected = cJSON_GetObjectItemCaseSensitive(session->request, "controls");
    if (session->selected_count != (size_t) cJSON_GetArraySize(selected))
        return fail("MODEL_GOOSE", "selected control block missing from converted model");
    session->active_controls = Json_allocate((session->selected_count + 1) * sizeof(MmsGooseControlBlock));
    memset(session->active_controls, 0, (session->selected_count + 1) * sizeof(MmsGooseControlBlock));
    return true;
}

static bool prepare_step(ModelSession* session, const cJSON* values, Step* step)
{
    if (!cJSON_IsArray(values)) return fail("MODEL_VALUES", "values must be an array");
    step->count = (size_t) cJSON_GetArraySize(values);
    step->updates = Json_allocate((step->count + 1) * sizeof(Update));
    memset(step->updates, 0, (step->count + 1) * sizeof(Update));
    size_t i = 0;
    const cJSON* item;
    cJSON_ArrayForEach(item, values) {
        Update* update = &step->updates[i++];
        update->reference = Json_string(item, "reference");
        if (!update->reference) return fail("MODEL_REFERENCE", "missing attribute reference");
        ModelNode* node = IedModel_getModelNodeByObjectReference(session->model, update->reference);
        if (!node || node->modelType != DataAttributeModelType)
            return fail("MODEL_REFERENCE", update->reference);
        update->attribute = (DataAttribute*) node;
        update->value = Values_prepare(update->attribute, cJSON_GetObjectItemCaseSensitive(item, "value"));
        if (!update->value) return fail("MODEL_VALUE", update->reference);
    }
    return true;
}

static bool prepare_timeline(ModelSession* session)
{
    if (!prepare_step(session, cJSON_GetObjectItemCaseSensitive(session->request,"initial"), &session->initial)) return false;
    const cJSON* timeline = cJSON_GetObjectItemCaseSensitive(session->request,"timeline");
    if (!cJSON_IsArray(timeline)) return fail("TIMELINE", "timeline must be an array");
    session->step_count = (size_t) cJSON_GetArraySize(timeline);
    session->steps = Json_allocate((session->step_count + 1) * sizeof(Step));
    memset(session->steps, 0, (session->step_count + 1) * sizeof(Step));
    const cJSON* item;
    size_t i = 0;
    cJSON_ArrayForEach(item, timeline) {
        Step* step = &session->steps[i++];
        if (!Json_u32(item,"at_ms",&step->at_ms)) return fail("TIMELINE", "invalid at_ms");
        if (i > 1 && session->steps[i-2].at_ms >= step->at_ms) return fail("TIMELINE", "times must increase");
        if (!prepare_step(session, cJSON_GetObjectItemCaseSensitive(item,"values"), step)) return false;
    }
    return true;
}

static void apply_step(ModelSession* session, Step* step)
{
    IedServer_lockDataModel(session->server);
    for (size_t i = 0; i < step->count; i++)
        IedServer_updateAttributeValue(session->server, step->updates[i].attribute, step->updates[i].value);
    IedServer_unlockDataModel(session->server);
}

static void goose_event(MmsGooseControlBlock cb, int event, void* parameter)
{
    ModelSession* session = parameter;
    Semaphore_wait(session->control_lock);
    size_t i = 0;
    for (GSEControlBlock* model = session->model->gseCBs; model; model = model->sibling) {
        if (strcmp(model->name, MmsGooseControlBlock_getName(cb)) == 0
            && model->parent == MmsGooseControlBlock_getLogicalNode(cb))
            session->active_controls[i] = cb;
        i++;
    }
    Semaphore_post(session->control_lock);
}

static bool create_server(ModelSession* session)
{
    IedServerConfig config = IedServerConfig_create();
    if (!config) return fail("MODEL_CREATE", "cannot allocate server configuration");
    IedServerConfig_enableFileService(config, false);
    IedServerConfig_useIntegratedGoosePublisher(config, session->selected_count > 0);
    session->server = IedServer_createWithConfig(session->model, NULL, config);
    IedServerConfig_destroy(config);
    if (!session->server) return fail("MODEL_CREATE", "cannot create IedServer");
    IedServer_setGoCBHandler(session->server, goose_event, session);
    // Default deny remains in force; the optional exact-attribute policy installs specific handlers.
    for (int fc = IEC61850_FC_ST; fc <= IEC61850_FC_US; fc++)
        IedServer_setWriteAccessPolicy(session->server, (FunctionalConstraint) fc, ACCESS_POLICY_DENY);
    return true;
}

ModelSession* Model_prepare(const cJSON* request)
{
    const char* path = Json_string(request,"model");
    const char* ied = Json_string(request,"ied");
    if (!path || !ied) { fail("MODEL_INPUT", "missing model/ied"); return NULL; }
    ModelSession* session = Json_allocate(sizeof(*session));
    memset(session, 0, sizeof(*session)); session->request = request;
    session->control_lock = Semaphore_create(1);
    session->model = ConfigFileParser_createModelFromConfigFileEx(path);
    if (!session->control_lock || !session->model) { fail("MODEL_LOAD", path); Model_destroy(session); return NULL; }
    if (strcmp(session->model->name, ied)) { fail("MODEL_IED", "converted IED does not match selection"); Model_destroy(session); return NULL; }
    if (!select_controls(session) || !create_server(session) || !prepare_timeline(session)) {
        Model_destroy(session); return NULL;
    }
    session->write_policy = Writes_create(session->server,session->model,cJSON_GetObjectItemCaseSensitive(request,"mms"));
    if (!session->write_policy) { Model_destroy(session); return NULL; }
    apply_step(session, &session->initial);
    return session;
}

static bool start_mms(ModelSession* session)
{
    const cJSON* mms = cJSON_GetObjectItemCaseSensitive(session->request, "mms");
    if (cJSON_IsNull(mms)) return true;
    const char* ip = Json_string(mms,"listen");
    uint32_t port;
    if (!ip || !Json_u32(mms,"port",&port) || port == 0 || port > 65535)
        return fail("MMS_CONFIG", "invalid listen address/port");
    IedServer_setLocalIpAddress(session->server, ip);
    IedServer_start(session->server, (int) port);
    session->mms_started = true;
    /* IedServer_isRunning also covers a GOOSE/event thread; inspect the actual MMS listener. */
    if (!MmsServer_isRunning(IedServer_getMmsServer(session->server)))
        return fail("MMS_LISTEN", "server failed to bind/listen");
    return true;
}

static bool start_goose(ModelSession* session)
{
    if (session->selected_count == 0) return true;
    session->interface = Platform_resolve_interface(Json_string(session->request,"interface"));
    if (!session->interface) return fail("GOOSE_INTERFACE", "unknown interface ID or no Ethernet MAC address");
    IedServer_setGooseInterfaceId(session->server, session->interface);
    for (GSEControlBlock* cb = session->model->gseCBs; cb; cb = cb->sibling) {
        const cJSON* selection = control_selection(session,cb);
        const cJSON* address = cJSON_GetObjectItemCaseSensitive(selection,"address");
        IedServer_useGooseVlanTag(session->server, cb->parent, cb->name,
            cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(address,"vlan_tagged")));
    }
    if (session->mms_started) IedServer_enableGoosePublishing(session->server);
    else IedServer_startGoosePublishing(session->server);
    session->goose_started = true;
    if (!Model_healthy(session)) return fail("GOOSE_START", "one or more publishers failed to enable");
    return true;
}

bool Model_start(ModelSession* session)
{
    return start_mms(session) && start_goose(session);
}

bool Model_healthy(ModelSession* session)
{
    if (session->mms_started && !MmsServer_isRunning(IedServer_getMmsServer(session->server))) return false;
    for (size_t i = 0; i < session->selected_count; i++) {
        Semaphore_wait(session->control_lock);
        MmsGooseControlBlock cb = session->active_controls[i];
        Semaphore_post(session->control_lock);
        if (!cb || !MmsGooseControlBlock_getGoEna(cb)) return false;
    }
    return true;
}

void Model_apply_due(ModelSession* session, uint64_t elapsed_ms)
{
    while (session->next_step < session->step_count) {
        Step* step = &session->steps[session->next_step];
        if (step->at_ms > elapsed_ms) break;
        apply_step(session, step);
        cJSON* event = cJSON_CreateObject();
        Json_add_u64(event,"at_ms",step->at_ms);
        Json_add_u64(event,"elapsed_ms",elapsed_ms);
        Json_add_u64(event,"count",step->count);
        Json_emit("update",event);
        session->next_step++;
    }
}

static void free_step(Step* step)
{
    if (!step->updates) return;
    for (size_t i = 0; i < step->count; i++) {
        if (step->updates[i].value) MmsValue_delete(step->updates[i].value);
    }
    free(step->updates);
}

void Model_destroy(ModelSession* session)
{
    if (!session) return;
    if (session->server) {
        if (session->mms_started) {
            if (session->goose_started) IedServer_disableGoosePublishing(session->server);
            IedServer_stop(session->server);
        } else if (session->goose_started) IedServer_stopGoosePublishing(session->server);
        IedServer_destroy(session->server);
    }
    Writes_destroy(session->write_policy);
    free_step(&session->initial);
    if (session->steps) {
        for (size_t i = 0; i < session->step_count; i++) free_step(&session->steps[i]);
        free(session->steps);
    }
    if (session->original_controls) {
        for (size_t i = 0; i < session->original_count; i++)
            session->original_controls[i]->sibling = i+1 < session->original_count ? session->original_controls[i+1] : NULL;
        session->model->gseCBs = session->original_count ? session->original_controls[0] : NULL;
    }
    if (session->model) IedModel_destroy(session->model);
    free(session->original_controls); free(session->active_controls); free(session->interface);
    if (session->control_lock) Semaphore_destroy(session->control_lock);
    free(session);
}
