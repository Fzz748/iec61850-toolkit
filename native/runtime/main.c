/* SPDX-License-Identifier: Apache-2.0 */
#include "runtime.h"
#include <stdio.h>
#include <string.h>
#include "json.h"

int main(int argc, char** argv)
{
    if (!Json_initialize()) { fputs("cannot initialize protocol output\n", stderr); return 2; }
    if (argc != 3 || strcmp(argv[1], "--request") != 0) {
        Json_error("ARGUMENT", "usage: iec61850-runtime --request FILE");
        Json_close(); return 2;
    }
    cJSON* request = Json_read_file(argv[2]);
    if (!request) { Json_error("REQUEST", "cannot read a complete JSON request"); Json_close(); return 2; }
    int result = Runtime_execute(request);
    cJSON_Delete(request);
    Json_close();
    return result;
}
