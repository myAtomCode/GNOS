cmake_minimum_required(VERSION 3.16)
project(FastProxy VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

add_compile_options(-Wall -Wextra)

add_library(fastproxy_core STATIC
  src/ir.cpp
  src/parser.cpp
  src/generator.cpp
  src/net.cpp
  src/adapter.cpp
  src/tls.cpp
)
target_include_directories(fastproxy_core PUBLIC src vendor)
find_package(OpenSSL REQUIRED)
target_link_libraries(fastproxy_core PUBLIC OpenSSL::SSL OpenSSL::Crypto)

add_executable(fastproxy
  src/main.cpp
  src/proxy.cpp
  src/reverse.cpp
  src/adapt.cpp
)
target_link_libraries(fastproxy PRIVATE fastproxy_core)
find_package(Threads REQUIRED)
target_link_libraries(fastproxy PRIVATE Threads::Threads)

enable_testing()
add_executable(fp_tests_ir tests/test_ir.cpp)
target_link_libraries(fp_tests_ir PRIVATE fastproxy_core)
add_test(NAME fp_tests_ir COMMAND fp_tests_ir)

add_executable(fp_tests_parser tests/test_parser.cpp)
target_link_libraries(fp_tests_parser PRIVATE fastproxy_core)
add_test(NAME fp_tests_parser COMMAND fp_tests_parser)
