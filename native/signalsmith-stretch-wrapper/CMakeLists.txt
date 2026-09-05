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

project(sonalloy-signalsmith-stretch-wrapper LANGUAGES CXX)

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

set(SONALLOY_SIGNALSMITH_STRETCH_REVISION "57b93f4e9206a089a45387eaa39bdc9f310d3308"
    CACHE INTERNAL "Vendored Signalsmith Stretch revision")
set(SONALLOY_SIGNALSMITH_LINEAR_REVISION "5668673560146a9cfe38c25315071e3fd68c8317"
    CACHE INTERNAL "Vendored Signalsmith Linear revision")

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

add_library(sonalloy_signalsmith_stretch_wrapper STATIC
    src/signalsmith_stretch_wrapper.cpp
)

target_compile_features(sonalloy_signalsmith_stretch_wrapper PRIVATE cxx_std_17)
target_include_directories(sonalloy_signalsmith_stretch_wrapper
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include
    PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/third_party/signalsmith-stretch
        ${CMAKE_CURRENT_SOURCE_DIR}/third_party/signalsmith-linear
        ${CMAKE_CURRENT_SOURCE_DIR}/third_party/signalsmith-linear/include
)

if(SONALLOY_DSP_TEST_HOOKS)
    target_compile_definitions(sonalloy_signalsmith_stretch_wrapper PRIVATE SONALLOY_DSP_TEST_HOOKS)
endif()

install(TARGETS sonalloy_signalsmith_stretch_wrapper
    ARCHIVE DESTINATION lib
    LIBRARY DESTINATION lib
    RUNTIME DESTINATION bin
)
