# On Windows SO_REUSEADDR allows a second listener to steal an occupied endpoint.
# Use exclusive ownership for the simulator and propagate option failures as socket creation errors.
if(WIN32)
    set(socket_original_source "${LIBIEC61850_SOURCE_DIR}/hal/socket/win32/socket_win32.c")
    file(READ "${socket_original_source}" socket_source)
    set(socket_original [=[    int optionReuseAddr = 1;
    setsockopt(listen_socket, SOL_SOCKET, SO_REUSEADDR, (char*)&optionReuseAddr, sizeof(int));]=])
    set(socket_exclusive [=[    int optionExclusiveAddr = 1;
    if (setsockopt(listen_socket, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
            (char*)&optionExclusiveAddr, sizeof(optionExclusiveAddr)) == SOCKET_ERROR)
    {
        closesocket(listen_socket);
        wsaShutdown();
        return NULL;
    }]=])
    string(FIND "${socket_source}" "${socket_original}" socket_patch_position)
    if(socket_patch_position EQUAL -1)
        message(FATAL_ERROR "Windows socket patch does not match the documented upstream revision")
    endif()
    string(REPLACE "${socket_original}" "${socket_exclusive}" socket_patched "${socket_source}")
    set(socket_generated "${CMAKE_CURRENT_BINARY_DIR}/socket_win32_exclusive.c")
    file(WRITE "${socket_generated}" "${socket_patched}")
    foreach(socket_target hal hal-shared)
        get_target_property(socket_sources ${socket_target} SOURCES)
        set(socket_candidates "${socket_sources}")
        list(FILTER socket_candidates INCLUDE REGEX "(^|/)socket_win32\\.c$")
        list(LENGTH socket_candidates socket_count)
        if(NOT socket_count EQUAL 1)
            message(FATAL_ERROR "Cannot identify the Windows socket compilation unit")
        endif()
        list(GET socket_candidates 0 socket_entry)
        list(REMOVE_ITEM socket_sources "${socket_entry}")
        list(APPEND socket_sources "${socket_generated}")
        set_property(TARGET ${socket_target} PROPERTY SOURCES "${socket_sources}")
    endforeach()
endif()
