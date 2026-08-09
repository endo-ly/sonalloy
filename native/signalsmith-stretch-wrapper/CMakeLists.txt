cmake_minimum_required(VERSION 3.14)

project(sonalloy-signalsmith-stretch-wrapper LANGUAGES CXX)

option(SONALLOY_DSP_TEST_HOOKS "Build internal native fault-injection hooks" OFF)

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
