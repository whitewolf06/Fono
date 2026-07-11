cmake_minimum_required(VERSION 3.21)

project(fono_stt_vulkan_worker LANGUAGES C CXX)

# Keep this worker self-contained: the application only needs one executable,
# while Vulkan itself remains dynamically resolved by the graphics driver.
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(WHISPER_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN ON CACHE BOOL "" FORCE)

add_subdirectory(
    "${CMAKE_CURRENT_LIST_DIR}/../../vendor/whisper.cpp"
    "${CMAKE_BINARY_DIR}/whispercpp"
)

# The worker receives redirected stdin/stdout from Fono; it is not an
# interactive terminal program and must not create a console window.
add_executable(fono-stt-vulkan-worker WIN32 src/main.cpp)
target_compile_features(fono-stt-vulkan-worker PRIVATE cxx_std_17)
target_include_directories(fono-stt-vulkan-worker PRIVATE
    "${CMAKE_CURRENT_LIST_DIR}/../../vendor/whisper.cpp/include"
    "${CMAKE_CURRENT_LIST_DIR}/../../vendor/whisper.cpp/examples"
)
target_link_libraries(fono-stt-vulkan-worker PRIVATE whisper)

# Keep the normal `int main()` entry point while linking as a GUI executable.
if (MSVC)
    target_link_options(fono-stt-vulkan-worker PRIVATE "/ENTRY:mainCRTStartup")
endif()
