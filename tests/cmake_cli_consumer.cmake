if(NOT DEFINED SONALLOY_CLI OR NOT EXISTS "${SONALLOY_CLI}")
    message(FATAL_ERROR "SONALLOY_CLI must point to a built executable")
endif()

execute_process(
    COMMAND "${SONALLOY_CLI}" --version
    RESULT_VARIABLE command_result
    OUTPUT_VARIABLE command_output
    ERROR_VARIABLE command_error
)
if(NOT command_result STREQUAL "0")
    message(FATAL_ERROR
        "Sonalloy CLI --version failed (exit ${command_result})\n"
        "stdout:\n${command_output}\n"
        "stderr:\n${command_error}")
endif()
