cmake_minimum_required(VERSION 3.15)

if(DEFINED ENV{MACOSX_DEPLOYMENT_TARGET} AND
   NOT DEFINED CMAKE_OSX_DEPLOYMENT_TARGET)
    set(CMAKE_OSX_DEPLOYMENT_TARGET "$ENV{MACOSX_DEPLOYMENT_TARGET}" CACHE STRING
        "Minimum macOS deployment target")
endif()

if(NOT DEFINED CMAKE_MSVC_RUNTIME_LIBRARY)
    set(CMAKE_MSVC_RUNTIME_LIBRARY
        "MultiThreaded$<$<CONFIG:Debug>:Debug>DLL")
endif()

project(sonalloy-daisysp-wrapper LANGUAGES CXX)

option(SONALLOY_DSP_TEST_HOOKS "Build internal native fault-injection hooks" OFF)

if(MSVC)
    if(DEFINED SONALLOY_MSVC_ITERATOR_DEBUG_LEVEL)
        add_compile_definitions("_ITERATOR_DEBUG_LEVEL=${SONALLOY_MSVC_ITERATOR_DEBUG_LEVEL}")
    else()
        add_compile_definitions(
            "$<$<CONFIG:Debug>:_ITERATOR_DEBUG_LEVEL=2>"
            "$<$<NOT:$<CONFIG:Debug>>:_ITERATOR_DEBUG_LEVEL=0>"
        )
    endif()
endif()

include(FetchContent)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(POLICY CMP0169)
    cmake_policy(SET CMP0169 OLD)
endif()

FetchContent_Declare(
    daisysp
    GIT_REPOSITORY https://github.com/electro-smith/DaisySP.git
    GIT_TAG a0494a3adb67f549e18dfd71a35fa656f65b38b6
    GIT_SHALLOW FALSE
)
FetchContent_Populate(daisysp)

# The wrapper consumes only the DaisySP modules required by Sonalloy. Building the upstream
# aggregate target would also compile unrelated modules that are outside the wrapper's scope.
add_library(DaisySP STATIC
    ${daisysp_SOURCE_DIR}/Source/Synthesis/oscillator.cpp
    ${daisysp_SOURCE_DIR}/Source/Synthesis/variableshapeosc.cpp
    ${daisysp_SOURCE_DIR}/Source/Filters/svf.cpp
    ${daisysp_SOURCE_DIR}/Source/Effects/wavefolder.cpp
    ${daisysp_SOURCE_DIR}/Source/PhysicalModeling/resonator.cpp
)
target_compile_features(DaisySP PUBLIC cxx_std_14)
target_include_directories(DaisySP PUBLIC
    ${daisysp_SOURCE_DIR}/Source
    ${daisysp_SOURCE_DIR}/Source/Synthesis
    ${daisysp_SOURCE_DIR}/Source/Utility
)

add_library(sonalloy_daisysp_wrapper STATIC
    src/daisysp_wrapper.cpp
)

target_compile_features(sonalloy_daisysp_wrapper PRIVATE cxx_std_14)
if(SONALLOY_DSP_TEST_HOOKS)
    target_compile_definitions(sonalloy_daisysp_wrapper PRIVATE SONALLOY_DSP_TEST_HOOKS=1)
endif()
target_include_directories(sonalloy_daisysp_wrapper
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>
)
target_link_libraries(sonalloy_daisysp_wrapper PRIVATE DaisySP)
set_target_properties(sonalloy_daisysp_wrapper PROPERTIES POSITION_INDEPENDENT_CODE ON)

install(
    TARGETS sonalloy_daisysp_wrapper DaisySP
    ARCHIVE DESTINATION lib
    LIBRARY DESTINATION lib
    RUNTIME DESTINATION bin
)
install(
    DIRECTORY include/
    DESTINATION include
)
