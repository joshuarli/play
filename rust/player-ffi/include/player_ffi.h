#ifndef PLAYER_FFI_H
#define PLAYER_FFI_H

#include <stddef.h>
#include <stdint.h>

enum player_ffi_input_result {
    PLAYER_FFI_INPUT_LOCAL = 0,
    PLAYER_FFI_INPUT_REJECTED = 1,
    PLAYER_FFI_INPUT_INVALID = 2,
    PLAYER_FFI_INPUT_BUFFER_TOO_SMALL = 3,
};

uint32_t player_ffi_abi_version(void);

int32_t player_ffi_classify_input(
    const char *path, int allow_file_uri,
    char *out_path, size_t out_path_cap,
    char *err, size_t err_cap);

int32_t player_ffi_panic_for_test(void);

#endif
