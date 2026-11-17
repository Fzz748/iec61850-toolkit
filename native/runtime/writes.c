/* SPDX-License-Identifier: Apache-2.0 */
#include "writes.h"
#include <stdlib.h>
#include <string.h>
#include "hal_time.h"
#include "json.h"
#include "values.h"

typedef struct {
    IedServer server;
    DataAttribute* attribute;
    const char* reference;
} WriteBinding;

struct WritePolicy {
    size_t count;
    WriteBinding* bindings;
};

static bool writable_fc(FunctionalConstraint fc)
{
    return fc == IEC61850_FC_SP || fc == IEC61850_FC_SV || fc == IEC61850_FC_CF
        || fc == IEC61850_FC_DC || fc == IEC61850_FC_BL;
}

static cJSON* write_event(const WriteBinding* binding, ClientConnection connection)
{
    cJSON* event = cJSON_CreateObject();
    cJSON_AddStringToObject(event,"reference",binding->reference);
    cJSON_AddStringToObject(event,"fc",FunctionalConstraint_toString(binding->attribute->fc));
    const char* peer = ClientConnection_getPeerAddress(connection);
    if (peer) cJSON_AddStringToObject(event,"peer",peer);
    Json_add_u64(event,"received_monotonic_ms",Hal_getMonotonicTimeInMs());
    return event;
}

static MmsDataAccessError accept_write(DataAttribute* attribute, MmsValue* value,
    ClientConnection connection, void* parameter)
{
    WriteBinding* binding = parameter;
    MmsValue* prepared = Values_prepare_received(attribute,value);
    cJSON* event = write_event(binding,connection);
    if (!prepared) {
        cJSON_AddStringToObject(event,"reason","value does not fit the model scalar type/range");
        Json_emit("mms_write_rejected",event);
        return DATA_ACCESS_ERROR_OBJECT_VALUE_INVALID;
    }
    /* The stack holds the model lock here; update without locking it a second time. */
    IedServer_updateAttributeValue(binding->server,attribute,prepared);
    MmsValue_delete(prepared);
    cJSON_AddItemToObject(event,"value",Values_json(attribute->mmsValue));
    Json_emit("mms_write",event);
    /* The real model update has happened; suppress the stack's duplicate update. */
    return DATA_ACCESS_ERROR_SUCCESS_NO_UPDATE;
}

static bool prepare_binding(WriteBinding* binding, IedModel* model, const cJSON* item)
{
    binding->reference = Json_string(item,"reference");
    const char* fc_name = Json_string(item,"fc");
    if (!binding->reference || !fc_name || strlen(fc_name) != 2) return false;
    FunctionalConstraint fc = FunctionalConstraint_fromString(fc_name);
    if (!writable_fc(fc)) return false;
    ModelNode* node = IedModel_getModelNodeByObjectReference(model,binding->reference);
    if (!node || node->modelType != DataAttributeModelType) return false;
    binding->attribute = (DataAttribute*) node;
    if (binding->attribute->fc != fc || !binding->attribute->mmsValue) return false;
    MmsValue* checked = Values_prepare_received(binding->attribute,binding->attribute->mmsValue);
    if (!checked) return false;
    MmsValue_delete(checked);
    return true;
}

WritePolicy* Writes_create(IedServer server, IedModel* model, const cJSON* mms)
{
    const cJSON* writable = cJSON_GetObjectItemCaseSensitive(mms,"writable");
    if (writable && !cJSON_IsArray(writable)) { Json_error("MMS_WRITABLE","writable must be an array"); return NULL; }
    WritePolicy* policy = Json_allocate(sizeof(*policy));
    policy->count = (size_t) cJSON_GetArraySize(writable);
    policy->bindings = Json_allocate((policy->count + 1) * sizeof(WriteBinding));
    memset(policy->bindings,0,(policy->count + 1) * sizeof(WriteBinding));
    for (size_t i = 0; i < policy->count; i++) {
        WriteBinding* binding = &policy->bindings[i];
        binding->server = server;
        if (!prepare_binding(binding,model,cJSON_GetArrayItem(writable,(int) i))) {
            Json_error("MMS_WRITABLE","reference must be a supported scalar leaf with matching SP/SV/CF/DC/BL");
            Writes_destroy(policy); return NULL;
        }
        for (size_t j = 0; j < i; j++) {
            if (policy->bindings[j].attribute == binding->attribute) {
                Json_error("MMS_WRITABLE","duplicate writable attribute"); Writes_destroy(policy); return NULL;
            }
        }
    }
    /* Install only after the entire configuration has validated. */
    for (size_t i = 0; i < policy->count; i++)
        IedServer_handleWriteAccess(server,policy->bindings[i].attribute,accept_write,&policy->bindings[i]);
    return policy;
}

void Writes_destroy(WritePolicy* policy)
{
    if (policy) { free(policy->bindings); free(policy); }
}
