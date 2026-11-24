cmake_minimum_required(VERSION 3.20)
project(LeafOpenHarmony LANGUAGES C CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
get_filename_component(SHINKI "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)
include("${SHINKI}/Tools/leaf-mobile/dependencies.cmake")
# XComponent presents the software canvas and OHAudio drives the engine mixer.
foreach(feature ALSA ARTS DBUS ESD HIDAPI IBUS JACK KMSDRM NAS OSS PIPEWIRE PULSEAUDIO RPI
                SNDIO UDEV VIVANTE VULKAN WAYLAND X11 OPENGL OPENGLES HAPTIC JOYSTICK SENSOR
                TEST TESTS CCACHE SHARED)
    set(SDL_${feature} OFF CACHE BOOL "" FORCE)
endforeach()
set(SDL_STATIC ON CACHE BOOL "" FORCE)
add_subdirectory("${UPSTREAM}/tools/SDL-release-2.32.10" sdl EXCLUDE_FROM_ALL)
target_compile_definitions(SDL2-static PRIVATE SDL_PLATFORM_OPENHARMONY=1)
add_subdirectory("${UPSTREAM}/engine" engine EXCLUDE_FROM_ALL)
add_library(wa2_movie STATIC "${UPSTREAM}/engine/movie_reader.cpp" "${UPSTREAM}/engine/movie_player.cpp")
target_link_libraries(wa2_movie PUBLIC wa2_engine leaf_mobile_codecs)
add_library(wa2_wave_codec STATIC "${UPSTREAM}/engine/wave_codec.cpp")
target_link_libraries(wa2_wave_codec PUBLIC wa2_engine leaf_mobile_codecs)
add_subdirectory("${UPSTREAM}/game" game EXCLUDE_FROM_ALL)
include("${UPSTREAM}/platform/native_services.cmake")
include("${UPSTREAM}/platform/sdl_canvas.cmake")
add_library(leaf_ohos SHARED player.cpp audio.cpp "${SHINKI}/Tools/leaf/adapter.cpp"
    "${UPSTREAM}/platform/sdl_presentation.cpp" "${UPSTREAM}/platform/sdl_application.cpp"
    "${SHINKI}/Platforms/OHOS/native/bridge.c")
target_include_directories(leaf_ohos PRIVATE "${SHINKI}/Source/FamilyApi/shinki-emu-abi/include")
target_link_libraries(leaf_ohos PRIVATE wa2_native_desktop_resources wa2_native_application wa2_native_audio_binding
    wa2_game_scene wa2_canvas wa2_movie wa2_wave_codec ace_napi.z ace_ndk.z native_window native_buffer native_vsync ohaudio hilog_ndk.z)
set_target_properties(leaf_ohos PROPERTIES OUTPUT_NAME shinki_emu_manager)
