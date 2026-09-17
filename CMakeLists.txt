cmake_minimum_required(VERSION 3.20)

if(DEFINED ENV{MACOSX_DEPLOYMENT_TARGET} AND
   NOT DEFINED CMAKE_OSX_DEPLOYMENT_TARGET)
    set(CMAKE_OSX_DEPLOYMENT_TARGET "$ENV{MACOSX_DEPLOYMENT_TARGET}" CACHE STRING
        "Minimum macOS deployment target")
endif()

if(NOT DEFINED CMAKE_MSVC_RUNTIME_LIBRARY)
    set(CMAKE_MSVC_RUNTIME_LIBRARY
        "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
endif()

project(sonalloy-native LANGUAGES CXX)

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(SONALLOY_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}" CACHE INTERNAL
    "Sonalloy source directory used by the public CMake integration")
find_program(SONALLOY_CARGO_EXECUTABLE cargo REQUIRED)
file(STRINGS "${SONALLOY_SOURCE_DIR}/Cargo.toml" SONALLOY_CARGO_VERSION_LINE
    REGEX "^[ \\t]*version[ \\t]*=[ \\t]*\"[^\"]+\"" LIMIT_COUNT 1)
string(REGEX REPLACE ".*\"([^\"]+)\".*" "\\1"
    SONALLOY_VERSION "${SONALLOY_CARGO_VERSION_LINE}")
if(NOT SONALLOY_VERSION MATCHES "^[0-9]+\\.[0-9]+\\.[0-9]+$")
    message(FATAL_ERROR "Sonalloy version could not be read from Cargo.toml")
endif()

file(GLOB_RECURSE SONALLOY_CAPI_INPUTS CONFIGURE_DEPENDS
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-capi/*"
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-core/*"
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-dsp-sys/*"
    "${SONALLOY_SOURCE_DIR}/native/*"
    "${SONALLOY_SOURCE_DIR}/Cargo.lock"
    "${SONALLOY_SOURCE_DIR}/Cargo.toml"
)

set(SONALLOY_BINARY_ROOT "${CMAKE_BINARY_DIR}")

if(MSVC)
    set(SONALLOY_CAPI_LIBRARY_NAME "sonalloy_capi.lib")
else()
    set(SONALLOY_CAPI_LIBRARY_NAME "libsonalloy_capi.a")
endif()

set(SONALLOY_CAPI_CONFIGURATION_DIRECTORY
    "${SONALLOY_BINARY_ROOT}/sonalloy-capi/$<CONFIG>")
set(SONALLOY_CAPI_LIBRARY
    "${SONALLOY_CAPI_CONFIGURATION_DIRECTORY}/${SONALLOY_CAPI_LIBRARY_NAME}")
set(SONALLOY_CAPI_CARGO_CONFIGURATION_DIRECTORY
    "${SONALLOY_BINARY_ROOT}/sonalloy-cargo-target/$<IF:$<CONFIG:Debug>,debug,release>")
set(SONALLOY_CAPI_CARGO_LIBRARY
    "${SONALLOY_CAPI_CARGO_CONFIGURATION_DIRECTORY}/${SONALLOY_CAPI_LIBRARY_NAME}")

add_custom_command(
    OUTPUT "${SONALLOY_CAPI_LIBRARY}"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SONALLOY_CAPI_CONFIGURATION_DIRECTORY}"
    COMMAND ${CMAKE_COMMAND} -E env
        "SONALLOY_DSP_MSVC_RUNTIME=$<IF:$<CONFIG:Debug>,debug,release>"
        "${SONALLOY_CARGO_EXECUTABLE}"
        rustc
        --manifest-path "${SONALLOY_SOURCE_DIR}/crates/sonalloy-capi/Cargo.toml"
        --package sonalloy-capi
        --profile "$<IF:$<CONFIG:Debug>,dev,release>"
        --lib
        --crate-type staticlib
        --target-dir "${SONALLOY_BINARY_ROOT}/sonalloy-cargo-target"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${SONALLOY_CAPI_CARGO_LIBRARY}"
        "${SONALLOY_CAPI_LIBRARY}"
    DEPENDS ${SONALLOY_CAPI_INPUTS}
    VERBATIM
)

add_custom_target(sonalloy-capi-build DEPENDS "${SONALLOY_CAPI_LIBRARY}")
add_library(sonalloy_capi STATIC IMPORTED GLOBAL)
set_target_properties(sonalloy_capi PROPERTIES
    IMPORTED_CONFIGURATIONS "DEBUG;RELWITHDEBINFO;RELEASE;MINSIZEREL"
    INTERFACE_INCLUDE_DIRECTORIES "${SONALLOY_SOURCE_DIR}/crates/sonalloy-capi/include"
)
foreach(sonalloy_configuration IN ITEMS Debug RelWithDebInfo Release MinSizeRel)
    string(TOUPPER "${sonalloy_configuration}" sonalloy_configuration_upper)
    set_target_properties(sonalloy_capi PROPERTIES
        "IMPORTED_LOCATION_${sonalloy_configuration_upper}"
            "${SONALLOY_BINARY_ROOT}/sonalloy-capi/${sonalloy_configuration}/${SONALLOY_CAPI_LIBRARY_NAME}"
    )
endforeach()
add_dependencies(sonalloy_capi sonalloy-capi-build)

if(MSVC)
    target_link_libraries(sonalloy_capi INTERFACE bcrypt advapi32 ntdll userenv ws2_32)
elseif(APPLE)
    target_link_libraries(sonalloy_capi INTERFACE c++)
elseif(UNIX)
    target_link_libraries(sonalloy_capi INTERFACE stdc++ m dl pthread)
endif()

add_library(sonalloy::capi ALIAS sonalloy_capi)

file(GLOB_RECURSE SONALLOY_CLI_INPUTS CONFIGURE_DEPENDS
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-cli/*"
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-core/*"
    "${SONALLOY_SOURCE_DIR}/crates/sonalloy-dsp-sys/*"
    "${SONALLOY_SOURCE_DIR}/native/*"
    "${SONALLOY_SOURCE_DIR}/Cargo.lock"
    "${SONALLOY_SOURCE_DIR}/Cargo.toml"
)

set(SONALLOY_CLI_CONFIGURATION_DIRECTORY
    "${SONALLOY_BINARY_ROOT}/sonalloy-cli/$<CONFIG>")
set(SONALLOY_CLI_EXECUTABLE_NAME "sonalloy${CMAKE_EXECUTABLE_SUFFIX}")
set(SONALLOY_CLI_EXECUTABLE
    "${SONALLOY_CLI_CONFIGURATION_DIRECTORY}/${SONALLOY_CLI_EXECUTABLE_NAME}")
set(SONALLOY_CLI_CARGO_EXECUTABLE
    "${SONALLOY_CAPI_CARGO_CONFIGURATION_DIRECTORY}/${SONALLOY_CLI_EXECUTABLE_NAME}")

add_custom_command(
    OUTPUT "${SONALLOY_CLI_EXECUTABLE}"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${SONALLOY_CLI_CONFIGURATION_DIRECTORY}"
    COMMAND ${CMAKE_COMMAND} -E env
        "SONALLOY_DSP_MSVC_RUNTIME=$<IF:$<CONFIG:Debug>,debug,release>"
        "${SONALLOY_CARGO_EXECUTABLE}"
        build
        --manifest-path "${SONALLOY_SOURCE_DIR}/crates/sonalloy-cli/Cargo.toml"
        --package sonalloy-cli
        --bin sonalloy
        --profile "$<IF:$<CONFIG:Debug>,dev,release>"
        --target-dir "${SONALLOY_BINARY_ROOT}/sonalloy-cargo-target"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${SONALLOY_CLI_CARGO_EXECUTABLE}"
        "${SONALLOY_CLI_EXECUTABLE}"
    DEPENDS ${SONALLOY_CLI_INPUTS}
    VERBATIM
)

add_custom_target(sonalloy-cli-build DEPENDS "${SONALLOY_CLI_EXECUTABLE}")
add_executable(sonalloy_cli IMPORTED GLOBAL)
set_target_properties(sonalloy_cli PROPERTIES
    IMPORTED_CONFIGURATIONS "DEBUG;RELWITHDEBINFO;RELEASE;MINSIZEREL"
)
foreach(sonalloy_configuration IN ITEMS Debug RelWithDebInfo Release MinSizeRel)
    string(TOUPPER "${sonalloy_configuration}" sonalloy_configuration_upper)
    set_target_properties(sonalloy_cli PROPERTIES
        "IMPORTED_LOCATION_${sonalloy_configuration_upper}"
            "${SONALLOY_BINARY_ROOT}/sonalloy-cli/${sonalloy_configuration}/${SONALLOY_CLI_EXECUTABLE_NAME}"
    )
endforeach()
add_dependencies(sonalloy_cli sonalloy-cli-build)

add_executable(sonalloy::cli ALIAS sonalloy_cli)

function(sonalloy_stage_builtin_bundle)
    include(CMakeParseArguments)
    set(options)
    set(one_value_arguments TARGET DESTINATION)
    set(multi_value_arguments)
    cmake_parse_arguments(STAGE "${options}" "${one_value_arguments}" "${multi_value_arguments}" ${ARGN})
    if(STAGE_UNPARSED_ARGUMENTS OR NOT STAGE_TARGET OR NOT STAGE_DESTINATION)
        message(FATAL_ERROR
            "sonalloy_stage_builtin_bundle requires TARGET and DESTINATION")
    endif()
    if(TARGET "${STAGE_TARGET}")
        message(FATAL_ERROR "Built-in bundle target already exists: ${STAGE_TARGET}")
    endif()

    find_program(SONALLOY_NODE_EXECUTABLE node REQUIRED)
    file(GLOB_RECURSE SONALLOY_BUNDLE_INPUTS CONFIGURE_DEPENDS
        "${SONALLOY_SOURCE_DIR}/presets/*"
        "${SONALLOY_SOURCE_DIR}/scripts/stage-builtin-bundle.mjs"
        "${SONALLOY_SOURCE_DIR}/Cargo.toml"
        "${SONALLOY_SOURCE_DIR}/LICENSE-*"
        "${SONALLOY_SOURCE_DIR}/THIRD_PARTY_NOTICES.md"
    )
    add_custom_target("${STAGE_TARGET}"
        COMMAND "${SONALLOY_NODE_EXECUTABLE}"
            "${SONALLOY_SOURCE_DIR}/scripts/stage-builtin-bundle.mjs"
            --destination "${STAGE_DESTINATION}"
            --source-root "${SONALLOY_SOURCE_DIR}"
        DEPENDS ${SONALLOY_BUNDLE_INPUTS}
        BYPRODUCTS
            "${STAGE_DESTINATION}/instruments/builtin/manifest.json"
            "${STAGE_DESTINATION}/THIRD_PARTY_NOTICES.md"
            "${STAGE_DESTINATION}/LICENSE-MIT"
            "${STAGE_DESTINATION}/LICENSE-APACHE"
        VERBATIM
    )
endfunction()

if(CMAKE_PROJECT_NAME STREQUAL PROJECT_NAME)
    include(CTest)
    if(BUILD_TESTING)
        add_executable(sonalloy-cmake-consumer tests/cmake_consumer.cpp)
        target_link_libraries(sonalloy-cmake-consumer PRIVATE sonalloy::capi)
        sonalloy_stage_builtin_bundle(
            TARGET sonalloy-builtin-bundle-stage
            DESTINATION "${CMAKE_CURRENT_BINARY_DIR}/test-builtin-resources"
        )
        add_dependencies(sonalloy-cmake-consumer sonalloy-builtin-bundle-stage)
        add_test(NAME SonalloyCMakeConsumer COMMAND sonalloy-cmake-consumer)
        add_test(
            NAME SonalloyBuiltinBundle
            COMMAND "${SONALLOY_NODE_EXECUTABLE}"
                "${SONALLOY_SOURCE_DIR}/scripts/verify-builtin-bundle.mjs"
                --root "${CMAKE_CURRENT_BINARY_DIR}/test-builtin-resources"
                --source-release "v${SONALLOY_VERSION}"
        )
    endif()
endif()

add_subdirectory(native/daisysp-wrapper)
