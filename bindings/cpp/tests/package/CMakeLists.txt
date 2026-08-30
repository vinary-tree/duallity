cmake_minimum_required(VERSION 3.20)
project(duallity_package_smoke LANGUAGES CXX)

find_package(duallity 4.0 CONFIG REQUIRED)
add_executable(duallity_package_smoke ../package_smoke.cpp)
target_compile_features(duallity_package_smoke PRIVATE cxx_std_20)
target_link_libraries(duallity_package_smoke PRIVATE duallity::duallity)
