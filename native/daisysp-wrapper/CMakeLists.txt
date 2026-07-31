cmake_minimum_required(VERSION 3.14)

project(sonalloy-daisysp-wrapper LANGUAGES CXX)

option(SONALLOY_DSP_TEST_HOOKS "Build internal native fault-injection hooks" OFF)

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

# The wrapper consumes only DaisySP's oscillator. Building the upstream aggregate target
# would also compile unrelated modules that are outside the wrapper's scope.
add_library(DaisySP STATIC
    ${daisysp_SOURCE_DIR}/Source/Synthesis/oscillator.cpp
    ${daisysp_SOURCE_DIR}/Source/Filters/svf.cpp
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
