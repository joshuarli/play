#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "player_ffi.h"

int main(void)
{
    assert(player_ffi_abi_version() == 1);

    char out[256];
    char err[256];
    int32_t rc = player_ffi_classify_input("/tmp/a:b.mp4", 0, out, sizeof(out),
                                           err, sizeof(err));
    assert(rc == PLAYER_FFI_INPUT_LOCAL);
    assert(strcmp(out, "/tmp/a:b.mp4") == 0);

    rc = player_ffi_classify_input("https://example.com/a.mp4", 0, out,
                                   sizeof(out), err, sizeof(err));
    assert(rc == PLAYER_FFI_INPUT_REJECTED);
    assert(strstr(err, "remote scheme") != NULL);

    rc = player_ffi_classify_input("", 0, out, sizeof(out), err, sizeof(err));
    assert(rc == PLAYER_FFI_INPUT_INVALID);

    rc = player_ffi_classify_input("/tmp/a.mp4", 0, out, 4, err, sizeof(err));
    assert(rc == PLAYER_FFI_INPUT_BUFFER_TOO_SMALL);

    assert(player_ffi_panic_for_test() == 1);

    puts("ffi ok");
    return 0;
}
