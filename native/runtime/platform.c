/* SPDX-License-Identifier: Apache-2.0 */
#include "platform.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "json.h"
#ifdef _WIN32
#include <winsock2.h>
#include <windows.h>
#include <iphlpapi.h>
#define HAVE_REMOTE
#include <pcap.h>
#else
#include <net/if.h>
#include <poll.h>
#include <unistd.h>
#endif

#ifdef _WIN32
static const char* interface_id(const char* pcap_name)
{
    /* findalldevs_ex prefixes local device paths with its transport URL. */
    return strncmp(pcap_name,"rpcap://",8) == 0 ? pcap_name + 8 : pcap_name;
}
#endif

bool Platform_stop_requested(void)
{
#ifdef _WIN32
    DWORD available = 0;
    HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
    if (!PeekNamedPipe(input, NULL, 0, NULL, &available, NULL))
        return GetLastError() == ERROR_BROKEN_PIPE;
    return available > 0;
#else
    struct pollfd input = {STDIN_FILENO, POLLIN | POLLHUP, 0};
    return poll(&input, 1, 0) > 0;
#endif
}

cJSON* Platform_interfaces(void)
{
    cJSON* result = cJSON_CreateArray();
#ifdef _WIN32
    pcap_if_t* list = NULL;
    char error[PCAP_ERRBUF_SIZE];
    if (pcap_findalldevs_ex(PCAP_SRC_IF_STRING, NULL, &list, error) != 0) {
        Json_error("INTERFACES", error);
        cJSON_Delete(result);
        return NULL;
    }
    for (pcap_if_t* entry = list; entry; entry = entry->next) {
        cJSON* item = cJSON_CreateObject();
        cJSON_AddStringToObject(item, "id", interface_id(entry->name));
        cJSON_AddStringToObject(item, "description", entry->description ? entry->description : "");
        cJSON_AddItemToArray(result, item);
    }
    pcap_freealldevs(list);
#else
    struct if_nameindex* list = if_nameindex();
    if (!list) { cJSON_Delete(result); return NULL; }
    for (struct if_nameindex* entry = list; entry->if_index; entry++) {
        cJSON* item = cJSON_CreateObject();
        cJSON_AddStringToObject(item, "id", entry->if_name);
        cJSON_AddItemToArray(result, item);
    }
    if_freenameindex(list);
#endif
    return result;
}

#ifdef _WIN32
static bool has_mac(const char* id)
{
    ULONG size = 0;
    if (GetAdaptersAddresses(AF_UNSPEC, 0, NULL, NULL, &size) != ERROR_BUFFER_OVERFLOW)
        return false;
    IP_ADAPTER_ADDRESSES* list = Json_allocate(size);
    bool found = false;
    if (GetAdaptersAddresses(AF_UNSPEC, 0, NULL, list, &size) == NO_ERROR) {
        for (IP_ADAPTER_ADDRESSES* p = list; p; p = p->Next) {
            if (p->PhysicalAddressLength == 6 && strstr(id,p->AdapterName)) found = true;
        }
    }
    free(list);
    return found;
}
#endif

char* Platform_resolve_interface(const char* id)
{
    if (!id || !id[0]) return NULL;
#ifdef _WIN32
    pcap_if_t* list = NULL;
    char error[PCAP_ERRBUF_SIZE];
    if (!has_mac(id) || pcap_findalldevs_ex(PCAP_SRC_IF_STRING,NULL,&list,error) != 0) return NULL;
    int index = 0;
    char* result = NULL;
    for (pcap_if_t* p = list; p; p = p->next) {
        if (strcmp(id,interface_id(p->name)) == 0) {
            result = Json_allocate(32);
            snprintf(result,32,"%d",index);
            break;
        }
        index++;
    }
    pcap_freealldevs(list);
    return result;
#else
    if (if_nametoindex(id) == 0) return NULL;
    char* result = Json_allocate(strlen(id)+1);
    strcpy(result,id);
    return result;
#endif
}
