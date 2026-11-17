/* SPDX-License-Identifier: Apache-2.0 */
#ifndef TOOLKIT_MMS_H
#define TOOLKIT_MMS_H
#include "cJSON.h"
/** One bounded MMS browse/read operation. Returns nonzero on actual protocol failures. */
int Mms_execute(const cJSON* request);
#endif
