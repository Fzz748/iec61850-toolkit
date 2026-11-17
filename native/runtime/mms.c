/* SPDX-License-Identifier: Apache-2.0 */
#include "mms.h"
#include <stdio.h>
#include <string.h>
#include "iec61850_client.h"
#include "linked_list.h"
#include "mms_type_spec.h"
#include "json.h"
#include "values.h"

static bool client_error(IedClientError error)
{
    if (error == IED_ERROR_OK) return false;
    char message[128];
    snprintf(message,sizeof(message),"MMS service failed: %s (%d)",IedClientError_toString(error),(int) error);
    Json_error("MMS_SERVICE",message);
    return true;
}

static cJSON* read_value(IedConnection connection, const cJSON* request)
{
    const char* reference = Json_string(request,"reference");
    const char* fc_name = Json_string(request,"fc");
    if (!reference || !fc_name) { Json_error("MMS_ARGUMENT","missing reference/fc"); return NULL; }
    FunctionalConstraint fc = FunctionalConstraint_fromString(fc_name);
    if (fc == IEC61850_FC_NONE || fc == IEC61850_FC_ALL) { Json_error("MMS_FC","invalid FC"); return NULL; }
    IedClientError error;
    MmsValue* value = IedConnection_readObject(connection,&error,reference,fc);
    if (client_error(error)) { if (value) MmsValue_delete(value); return NULL; }
    cJSON* result = Values_json(value);
    if (value) MmsValue_delete(value);
    if (!result) Json_error("MMS_VALUE","unsupported or missing returned value");
    return result;
}

static cJSON* browse_device(IedConnection connection, const char* device)
{
    IedClientError error;
    LinkedList nodes = IedConnection_getLogicalDeviceDirectory(connection,&error,device);
    if (client_error(error)) { if (nodes) LinkedList_destroy(nodes); return NULL; }
    cJSON* result = cJSON_CreateObject();
    cJSON_AddStringToObject(result,"logical_device",device);
    cJSON* names = cJSON_AddArrayToObject(result,"logical_nodes");
    for (LinkedList item = LinkedList_getNext(nodes); item; item = LinkedList_getNext(item))
        cJSON_AddItemToArray(names,cJSON_CreateString(LinkedList_getData(item)));
    LinkedList_destroy(nodes);
    return result;
}

static cJSON* browse(IedConnection connection)
{
    IedClientError error;
    LinkedList devices = IedConnection_getServerDirectory(connection,&error,false);
    if (client_error(error)) { if (devices) LinkedList_destroy(devices); return NULL; }
    cJSON* result = cJSON_CreateArray();
    for (LinkedList item = LinkedList_getNext(devices); item; item = LinkedList_getNext(item)) {
        cJSON* device = browse_device(connection,LinkedList_getData(item));
        if (!device) { cJSON_Delete(result); LinkedList_destroy(devices); return NULL; }
        cJSON_AddItemToArray(result,device);
    }
    LinkedList_destroy(devices);
    return result;
}

static cJSON* write_value(IedConnection connection, const cJSON* request)
{
    const char* reference = Json_string(request,"reference");
    const char* fc_name = Json_string(request,"fc");
    if (!reference || !fc_name || strlen(fc_name) != 2) { Json_error("MMS_ARGUMENT","missing reference or invalid fc"); return NULL; }
    FunctionalConstraint fc = FunctionalConstraint_fromString(fc_name);
    if (fc == IEC61850_FC_NONE || fc == IEC61850_FC_ALL || fc == IEC61850_FC_CO) {
        Json_error("MMS_FC","requires a data FC; control/SBO/Operate are separate services"); return NULL;
    }
    IedClientError error;
    MmsVariableSpecification* spec = IedConnection_getVariableSpecification(connection,&error,reference,fc);
    if (client_error(error)) { if (spec) MmsVariableSpecification_destroy(spec); return NULL; }
    if (!spec) { Json_error("MMS_TYPE","no remote variable specification"); return NULL; }
    MmsValue* value = Values_from_spec(spec,cJSON_GetObjectItemCaseSensitive(request,"value"));
    MmsVariableSpecification_destroy(spec);
    if (!value) { Json_error("MMS_WRITE_VALUE","value does not fit the remote scalar type/range, or type is unsupported; no write sent"); return NULL; }
    cJSON* result = Values_json(value);
    if (!result) { MmsValue_delete(value); Json_error("MMS_WRITE_VALUE","cannot represent the write value; no write sent"); return NULL; }
    IedConnection_writeObject(connection,&error,reference,fc,value);
    MmsValue_delete(value);
    if (error != IED_ERROR_OK) {
        cJSON_Delete(result);
        if (error == IED_ERROR_TIMEOUT || error == IED_ERROR_CONNECTION_LOST)
            Json_error("MMS_WRITE_OUTCOME_UNKNOWN","write acknowledgement unavailable; the value may have been applied; no automatic retry");
        else client_error(error);
        return NULL;
    }
    return result;
}

static cJSON* perform(IedConnection connection, const cJSON* request)
{
    const char* action = Json_string(request,"action");
    if (strcmp(action,"read") == 0) return read_value(connection,request);
    if (strcmp(action,"write") == 0) return write_value(connection,request);
    return browse(connection);
}

int Mms_execute(const cJSON* request)
{
    uint32_t port, timeout;
    const char* host = Json_string(request,"host");
    const char* action = Json_string(request,"action");
    if (!host || !Json_u32(request,"port",&port) || !port || port > 65535
        || !Json_u32(request,"timeout_ms",&timeout) || !timeout) {
        Json_error("MMS_ARGUMENT","invalid host/port/timeout"); return 2;
    }
    IedConnection connection = IedConnection_create();
    if (!connection) { Json_error("MMS_CREATE","cannot create client"); return 2; }
    IedConnection_setConnectTimeout(connection,timeout);
    IedConnection_setRequestTimeout(connection,timeout);
    IedClientError error;
    IedConnection_connect(connection,&error,host,(int) port);
    if (client_error(error)) { IedConnection_destroy(connection); return 2; }
    cJSON* value = perform(connection,request);
    IedConnection_close(connection);
    IedConnection_destroy(connection);
    if (!value) return 2;
    cJSON* result = cJSON_CreateObject();
    cJSON_AddItemToObject(result,"value",value);
    cJSON_AddStringToObject(result,"host",host);
    cJSON_AddNumberToObject(result,"port",port);
    if (strcmp(action,"write") == 0) {
        cJSON_AddBoolToObject(result,"acknowledged",true);
        cJSON_AddStringToObject(result,"reference",Json_string(request,"reference"));
        cJSON_AddStringToObject(result,"fc",Json_string(request,"fc"));
    }
    Json_emit("result",result);
    return 0;
}
