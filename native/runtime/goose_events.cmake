# libiec61850 v1.6.2.1's enable-all path omits the public GoCB notification made by
# the MMS write path. Observe the actual successful enable result through that API.
# Generate a patched compilation unit in the build tree; never rewrite the supplied checkout.
set(goose_mapping_source "${LIBIEC61850_SOURCE_DIR}/src/iec61850/server/mms_mapping/mms_mapping.c")
file(READ "${goose_mapping_source}" goose_mapping)
set(goose_original [=[        if (MmsGooseControlBlock_enable(gcb, self) == false)
        {
            if (DEBUG_IED_SERVER)
                printf("IED_SERVER: failed to enable GoCB %s\n", MmsGooseControlBlock_getName(gcb));
        }
]=])
set(goose_notification [=[        if (MmsGooseControlBlock_enable(gcb, self) == false)
        {
            if (DEBUG_IED_SERVER)
                printf("IED_SERVER: failed to enable GoCB %s\n", MmsGooseControlBlock_getName(gcb));
        }
        else if (self->goCbHandler)
        {
            self->goCbHandler(gcb, IEC61850_GOCB_EVENT_ENABLE, self->goCbHandlerParameter);
        }
]=])
string(FIND "${goose_mapping}" "${goose_original}" goose_patch_position)
if(goose_patch_position EQUAL -1)
    message(FATAL_ERROR "GOOSE notification patch does not match; use the documented libiec61850 revision")
endif()
string(REPLACE "${goose_original}" "${goose_notification}" goose_patched "${goose_mapping}")
set(goose_generated "${CMAKE_CURRENT_BINARY_DIR}/mms_mapping_goose_events.c")
file(WRITE "${goose_generated}" "${goose_patched}")
foreach(goose_target iec61850 iec61850-shared)
    get_target_property(goose_sources ${goose_target} SOURCES)
    set(goose_candidates "${goose_sources}")
    list(FILTER goose_candidates INCLUDE REGEX "(^|/)mms_mapping\\.c$")
    list(LENGTH goose_candidates goose_candidate_count)
    if(NOT goose_candidate_count EQUAL 1)
        message(FATAL_ERROR "Cannot identify the upstream GOOSE compilation unit")
    endif()
    list(GET goose_candidates 0 goose_source_entry)
    list(REMOVE_ITEM goose_sources "${goose_source_entry}")
    list(APPEND goose_sources "${goose_generated}")
    set_property(TARGET ${goose_target} PROPERTY SOURCES "${goose_sources}")
endforeach()
