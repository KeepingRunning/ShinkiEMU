# SDL's UIKit sources require both Objective-C language rules before configuring its subdirectory.
if(PROJECT_NAME STREQUAL "shinki_leaf" AND NOT CMAKE_OBJCXX_COMPILER_LOADED)
    enable_language(OBJC OBJCXX)
endif()
