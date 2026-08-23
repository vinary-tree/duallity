include_guard(GLOBAL)

include(CMakeFindDependencyMacro)
find_dependency(Threads)
find_dependency(vinary-tree-interop 4.0 CONFIG)

get_filename_component(_DUALLITY_PREFIX "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)

if(NOT TARGET duallity::shared)
  add_library(duallity::shared SHARED IMPORTED)
  set_target_properties(duallity::shared PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_DUALLITY_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "vinary-tree::interop"
  )
  if(WIN32)
    set_target_properties(duallity::shared PROPERTIES
      IMPORTED_LOCATION "${_DUALLITY_PREFIX}/bin/duallity.dll"
      IMPORTED_IMPLIB "${_DUALLITY_PREFIX}/lib/duallity.dll.lib"
    )
  elseif(APPLE)
    set_target_properties(duallity::shared PROPERTIES IMPORTED_LOCATION "${_DUALLITY_PREFIX}/lib/libduallity.dylib")
  else()
    set_target_properties(duallity::shared PROPERTIES IMPORTED_LOCATION "${_DUALLITY_PREFIX}/lib/libduallity.so")
  endif()
endif()

if(NOT TARGET duallity::static)
  add_library(duallity::static STATIC IMPORTED)
  set_target_properties(duallity::static PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_DUALLITY_PREFIX}/include"
    INTERFACE_LINK_LIBRARIES "vinary-tree::interop"
  )
  if(WIN32)
    set_target_properties(duallity::static PROPERTIES
      IMPORTED_LOCATION "${_DUALLITY_PREFIX}/lib/duallity.lib"
      INTERFACE_LINK_LIBRARIES "bcrypt;userenv;ws2_32;ntdll;synchronization;advapi32;Threads::Threads"
    )
  elseif(APPLE)
    find_library(_DUALLITY_ICONV_LIBRARY NAMES iconv REQUIRED)
    find_library(_DUALLITY_COREFOUNDATION_FRAMEWORK NAMES CoreFoundation REQUIRED)
    find_library(_DUALLITY_SECURITY_FRAMEWORK NAMES Security REQUIRED)
    set_target_properties(duallity::static PROPERTIES
      IMPORTED_LOCATION "${_DUALLITY_PREFIX}/lib/libduallity.a"
      INTERFACE_LINK_LIBRARIES "${CMAKE_DL_LIBS};Threads::Threads;m;${_DUALLITY_ICONV_LIBRARY};${_DUALLITY_COREFOUNDATION_FRAMEWORK};${_DUALLITY_SECURITY_FRAMEWORK}"
    )
  else()
    set_target_properties(duallity::static PROPERTIES
      IMPORTED_LOCATION "${_DUALLITY_PREFIX}/lib/libduallity.a"
      INTERFACE_LINK_LIBRARIES "${CMAKE_DL_LIBS};Threads::Threads;m"
    )
  endif()
endif()

if(NOT DEFINED DUALLITY_LINKAGE)
  set(DUALLITY_LINKAGE "SHARED")
endif()
string(TOUPPER "${DUALLITY_LINKAGE}" _DUALLITY_LINKAGE)
if(NOT _DUALLITY_LINKAGE STREQUAL "SHARED" AND NOT _DUALLITY_LINKAGE STREQUAL "STATIC")
  message(FATAL_ERROR "DUALLITY_LINKAGE must be SHARED or STATIC")
endif()
if(NOT TARGET duallity::duallity)
  add_library(duallity::duallity INTERFACE IMPORTED)
  if(_DUALLITY_LINKAGE STREQUAL "STATIC")
    set_property(TARGET duallity::duallity PROPERTY INTERFACE_LINK_LIBRARIES duallity::static)
  else()
    set_property(TARGET duallity::duallity PROPERTY INTERFACE_LINK_LIBRARIES duallity::shared)
  endif()
endif()

set(duallity_FOUND TRUE)
unset(_DUALLITY_LINKAGE)
unset(_DUALLITY_PREFIX)
