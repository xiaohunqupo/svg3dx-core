cmake_minimum_required(VERSION 3.16)
project(svg3dx_cpp_examples LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_executable(dump_json examples/dump_json.cpp)
target_include_directories(dump_json PRIVATE include)

if(UNIX AND NOT APPLE)
  target_link_libraries(dump_json PRIVATE dl)
endif()

install(TARGETS dump_json RUNTIME DESTINATION bin)
