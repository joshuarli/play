#ifndef PLAYER_LOCAL_FILE_H
#define PLAYER_LOCAL_FILE_H

#include <stddef.h>
#include <stdint.h>

struct player_local_file;

struct player_local_file *player_ffi_local_file_open(const char *path, int write,
                                                     char *err, size_t cap);
int player_ffi_local_file_is_directory(const char *path, char *err, size_t cap);
int player_ffi_local_file_read(struct player_local_file *file, uint8_t *buffer,
                               size_t length, size_t *out_read,
                               char *err, size_t cap);
int player_ffi_local_file_write(struct player_local_file *file,
                                const uint8_t *buffer, size_t length,
                                size_t *out_written, char *err, size_t cap);
int player_ffi_local_file_seek(struct player_local_file *file, int64_t offset,
                               int whence, uint64_t *out_position,
                               char *err, size_t cap);
int player_ffi_local_file_size(struct player_local_file *file,
                               uint64_t *out_size, char *err, size_t cap);
void player_ffi_local_file_destroy(struct player_local_file *file);

#endif
