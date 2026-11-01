#ifndef PLAYER_COMMAND_H
#define PLAYER_COMMAND_H

#include <stddef.h>

int player_ffi_command_check_strv(int argc, const char **argv,
                                  char *err, size_t err_cap);
int player_ffi_command_check_str(const char *text,
                                 char *err, size_t err_cap);

#endif
