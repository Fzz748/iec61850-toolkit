/* SPDX-License-Identifier: Apache-2.0 */
#include "json.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "hal_thread.h"
#ifdef _WIN32
#include <io.h>
#define dup _dup
#define dup2 _dup2
#define fileno _fileno
#define fdopen _fdopen
#else
#include <unistd.h>
#endif

static FILE* protocol;
static Semaphore output_lock;
static uint32_t protocol_version = 1;

void Json_set_protocol_version(uint32_t version)
{
    protocol_version = version;
}

void* Json_allocate(size_t size)
{
    void* result = malloc(size);
    if (!result) {
        fputs("runtime allocation failed\n", stderr);
        exit(2);
    }
    return result;
}

bool Json_initialize(void)
{
    cJSON_Hooks hooks = {Json_allocate, free};
    cJSON_InitHooks(&hooks);
    int descriptor = dup(fileno(stdout));
    if (descriptor < 0) return false;
    protocol = fdopen(descriptor, "w");
    if (!protocol || dup2(fileno(stderr), fileno(stdout)) < 0) return false;
    output_lock = Semaphore_create(1);
    return output_lock != NULL;
}

void Json_close(void)
{
    if (fclose(protocol) != 0) fputs("cannot close protocol output\n", stderr);
    Semaphore_destroy(output_lock);
}

cJSON* Json_read_file(const char* path)
{
    FILE* input = fopen(path, "rb");
    if (!input) return NULL;
    if (fseek(input, 0, SEEK_END) != 0) { fclose(input); return NULL; }
    long size = ftell(input);
    if (size < 0 || fseek(input, 0, SEEK_SET) != 0) { fclose(input); return NULL; }
    char* text = Json_allocate((size_t) size + 1);
    size_t length = fread(text, 1, (size_t) size, input);
    bool valid = length == (size_t) size && !ferror(input);
    if (fclose(input) != 0) valid = false;
    text[length] = 0;
    cJSON* result = valid ? cJSON_ParseWithLengthOpts(text, length + 1, NULL, true) : NULL;
    free(text);
    return result;
}

const char* Json_string(const cJSON* object, const char* name)
{
    const cJSON* item = cJSON_GetObjectItemCaseSensitive(object, name);
    return cJSON_IsString(item) ? item->valuestring : NULL;
}

bool Json_number(const cJSON* value, double min, double max)
{
    return cJSON_IsNumber(value) && isfinite(value->valuedouble)
        && value->valuedouble >= min && value->valuedouble <= max;
}

bool Json_u32(const cJSON* object, const char* name, uint32_t* value)
{
    const cJSON* number = cJSON_GetObjectItemCaseSensitive(object, name);
    if (!Json_number(number, 0, UINT32_MAX) || floor(number->valuedouble) != number->valuedouble)
        return false;
    *value = (uint32_t) number->valuedouble;
    return true;
}

void Json_add_u64(cJSON* object, const char* key, uint64_t value)
{
    cJSON_AddNumberToObject(object, key, (double) value);
}

void Json_emit(const char* event, cJSON* data)
{
    cJSON* envelope = cJSON_CreateObject();
    cJSON_AddNumberToObject(envelope, "protocol_version", protocol_version);
    cJSON_AddStringToObject(envelope, "event", event);
    cJSON_AddItemToObject(envelope, "data", data);
    char* line = cJSON_PrintUnformatted(envelope);
    Semaphore_wait(output_lock);
    bool failed = fputs(line, protocol) < 0 || fputc('\n', protocol) == EOF || fflush(protocol) != 0;
    Semaphore_post(output_lock);
    cJSON_free(line);
    cJSON_Delete(envelope);
    if (failed) { fputs("runtime event output failed\n", stderr); exit(2); }
}

void Json_error(const char* code, const char* message)
{
    cJSON* data = cJSON_CreateObject();
    cJSON_AddStringToObject(data, "code", code);
    cJSON_AddStringToObject(data, "message", message);
    Json_emit("error", data);
}
