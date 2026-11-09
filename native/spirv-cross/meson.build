project('SPIRV-Cross', 'cpp', 'c',
  version: '0.68.0',
  default_options: ['cpp_std=c++11', 'default_library=static', 'warning_level=0'],
  meson_version: '>=0.60.0',
)

cpp = meson.get_compiler('cpp')
add_project_arguments('-fPIC', language: ['cpp', 'c'])

inc = include_directories('.')
gitversion = configure_file(
  input: 'gitversion.h.in',
  output: 'gitversion.h',
  configuration: {'SPIRV_CROSS_GIT_REVISION': 'Git commit: unknown  Timestamp: unknown'},
  format: 'cmake@',
)

core_sources = files(
  'GLSL.std.450.h',
  'spirv_common.hpp',
  'spirv_cross_containers.hpp',
  'spirv_cross_error_handling.hpp',
  'spirv.hpp',
  'spirv_cross.hpp',
  'spirv_cross.cpp',
  'spirv_parser.hpp',
  'spirv_parser.cpp',
  'spirv_cross_parsed_ir.hpp',
  'spirv_cross_parsed_ir.cpp',
  'spirv_cfg.hpp',
  'spirv_cfg.cpp',
)

glsl_sources = files(
  'spirv_glsl.cpp',
  'spirv_glsl.hpp',
)

msl_sources = files(
  'spirv_msl.cpp',
  'spirv_msl.hpp',
)

c_sources = files(
  'spirv.h',
  'spirv_cross_c.cpp',
  'spirv_cross_c.h',
)

spirv_cross_core = static_library('spirv-cross-core',
  core_sources,
  include_directories: inc,
  pic: true,
  install: true,
)

spirv_cross_glsl = static_library('spirv-cross-glsl',
  glsl_sources,
  include_directories: inc,
  link_with: spirv_cross_core,
  pic: true,
  install: true,
)

spirv_cross_msl = static_library('spirv-cross-msl',
  msl_sources,
  include_directories: inc,
  link_with: [spirv_cross_core, spirv_cross_glsl],
  pic: true,
  install: true,
)

spirv_cross_c = static_library('spirv-cross-c',
  [c_sources, gitversion],
  include_directories: inc,
  cpp_args: [
    '-DHAVE_SPIRV_CROSS_GIT_VERSION',
    '-DSPIRV_CROSS_C_API_GLSL=1',
    '-DSPIRV_CROSS_C_API_MSL=1',
  ],
  link_with: [spirv_cross_core, spirv_cross_glsl, spirv_cross_msl],
  pic: true,
  install: true,
)

install_headers(
  'GLSL.std.450.h',
  'spirv_cfg.hpp',
  'spirv_common.hpp',
  'spirv_cross_c.h',
  'spirv_cross_containers.hpp',
  'spirv_cross_error_handling.hpp',
  'spirv_cross_parsed_ir.hpp',
  'spirv_cross.hpp',
  'spirv_glsl.hpp',
  'spirv_msl.hpp',
  'spirv_parser.hpp',
  'spirv.h',
  'spirv.hpp',
  subdir: 'spirv_cross',
)

pkg = import('pkgconfig')
pkg.generate(
  spirv_cross_c,
  name: 'spirv-cross-c',
  description: 'C API for SPIRV-Cross (static)',
  subdirs: 'spirv_cross',
  libraries: spirv_cross_c,
)
