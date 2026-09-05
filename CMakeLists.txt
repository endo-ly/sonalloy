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

project(sonalloy-native LANGUAGES CXX)

set(CMAKE_POSITION_INDEPENDENT_CODE ON)
add_subdirectory(native/daisysp-wrapper)
