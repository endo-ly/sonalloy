if(NOT DEFINED SONALLOY_CLI OR NOT EXISTS "${SONALLOY_CLI}")
    message(FATAL_ERROR "SONALLOY_CLI must point to a built executable")
endif()
if(NOT DEFINED SONALLOY_CLI_SMOKE_DIRECTORY)
    message(FATAL_ERROR "SONALLOY_CLI_SMOKE_DIRECTORY is required")
endif()

file(REMOVE_RECURSE "${SONALLOY_CLI_SMOKE_DIRECTORY}")
file(MAKE_DIRECTORY "${SONALLOY_CLI_SMOKE_DIRECTORY}")
set(definition "${SONALLOY_CLI_SMOKE_DIRECTORY}/definition.json")

function(run_cli)
    execute_process(
        COMMAND "${SONALLOY_CLI}" ${ARGN}
        RESULT_VARIABLE command_result
        OUTPUT_VARIABLE command_output
        ERROR_VARIABLE command_error
    )
    if(NOT command_result STREQUAL "0")
        message(FATAL_ERROR
            "Sonalloy CLI command failed (exit ${command_result}): ${ARGN}\n"
            "stdout:\n${command_output}\n"
            "stderr:\n${command_error}")
    endif()
endfunction()

run_cli(--version)
run_cli(instrument init "${definition}")
run_cli(instrument validate "${definition}" --json)
run_cli(instrument inspect "${definition}" --json)

if(NOT EXISTS "${definition}")
    message(FATAL_ERROR "Sonalloy CLI did not create the Definition")
endif()
