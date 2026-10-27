# whisper-rs-sys forwards CMAKE_* environment variables to CMake. Apply the
# existing release flags only to MSVC and preserve explicit caller overrides.
if(MSVC)
  foreach(language C CXX)
    set(flag_name "CMAKE_${language}_FLAGS_RELEASE")
    if(NOT DEFINED ENV{${flag_name}})
      set(${flag_name} "/O2 /Ob2 /DNDEBUG" CACHE STRING "" FORCE)
    endif()
  endforeach()
endif()
