#ifdef _WIN32
#include "syswin.c"
#else
/* Bodies of the std platform intrinsics declared in sys.h (W-SHIM). */
#include <errno.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <dirent.h>
#include <sys/stat.h>
#ifdef __linux__
#include <sys/syscall.h>
#endif

extern char **environ;

int32_t lust_stdout_write_soft(const uint8_t *p, uint64_t n); /* panic.c */
int32_t lust_stdout_flush_soft(void);

int64_t lust_sys_write_std(int32_t fd, const uint8_t *p, uint64_t len) {
    FILE *f = fd == 2 ? stderr : stdout;
    if (len == 0) return 0;
    /* Stdout shares print!'s LineWriter (W-PANIC, panic.c). */
    if (fd != 2) {
        int32_t e = lust_stdout_write_soft(p, len);
        return e ? -(int64_t)e : (int64_t)len;
    }
    size_t n = fwrite(p, 1, (size_t)len, f);
    if (n < len) {
        int e = errno ? errno : 5; /* EIO when stdio gives no reason */
        clearerr(f);
        if (n == 0) return -(int64_t)e;
    }
    if (fd == 2 && fflush(stderr) != 0) {
        int e = errno ? errno : 5;
        clearerr(stderr);
        return -(int64_t)e;
    }
    return (int64_t)n;
}

int32_t lust_sys_flush_std(int32_t fd) {
    if (fd != 2) return lust_stdout_flush_soft();
    FILE *f = stderr;
    if (fflush(f) != 0) {
        int e = errno ? errno : 5;
        clearerr(f);
        return e;
    }
    return 0;
}

void lust_sys_os_error_text(int32_t code, lust_String *out) {
    lust_string_push_cstr(out, strerror(code));
}

lust_Vec lust_sys_fmt_arguments_bytes(lust_Arguments args) {
    lust_String s = lust_string_with_capacity(0);
    lust_sink k;
    lust_sink_string(&k, &s);
    if (lust_args_write(&args, &k)) lust_fmt_failed(2, 0);
    lust_Vec v;
    v.ptr = s.ptr;
    v.len = s.len;
    v.cap = s.cap;
    return v;
}

lust_str lust_sys_str_as_bytes(lust_str s) { return s; }

lust_str lust_sys_bytes_as_str(const uint8_t *p, uint64_t len) {
    lust_str s;
    s.ptr = (uint8_t *)p;
    s.len = len;
    return s;
}

lust_String lust_sys_string_from_utf8_unchecked(lust_Vec v) {
    lust_String s;
    s.ptr = (uint8_t *)v.ptr;
    s.len = v.len;
    s.cap = v.cap;
    return s;
}

/* The length of the valid UTF-8 sequence at p[i..], or 0 when it is
   invalid; `*bad` gets the length of the maximal invalid subpart then
   (core::str::Utf8Chunks: at least 1, at most 3). */
static uint64_t lust_sys_utf8_seq(const uint8_t *p, uint64_t i, uint64_t len, uint64_t *bad) {
    uint8_t b = p[i];
    *bad = 1;
    if (b < 0x80) return 1;
    uint64_t need;
    uint8_t lo = 0x80, hi = 0xBF;
    if (b >= 0xC2 && b <= 0xDF) need = 2;
    else if (b == 0xE0) { need = 3; lo = 0xA0; }
    else if (b >= 0xE1 && b <= 0xEC) need = 3;
    else if (b == 0xED) { need = 3; hi = 0x9F; }
    else if (b >= 0xEE && b <= 0xEF) need = 3;
    else if (b == 0xF0) { need = 4; lo = 0x90; }
    else if (b >= 0xF1 && b <= 0xF3) need = 4;
    else if (b == 0xF4) { need = 4; hi = 0x8F; }
    else return 0;
    for (uint64_t k = 1; k < need; k++) {
        if (i + k >= len) return 0;
        uint8_t c = p[i + k];
        if (c < (k == 1 ? lo : 0x80) || c > (k == 1 ? hi : 0xBF)) return 0;
        *bad = k + 1;
    }
    return need;
}

uint64_t lust_sys_utf8_valid_up_to(const uint8_t *p, uint64_t len) {
    uint64_t i = 0, bad;
    while (i < len) {
        uint64_t n = lust_sys_utf8_seq(p, i, len, &bad);
        if (n == 0) return i;
        i += n;
    }
    return len;
}

void lust_sys_utf8_lossy(const uint8_t *p, uint64_t len, lust_String *out) {
    uint64_t i = 0, start = 0, bad;
    while (i < len) {
        uint64_t n = lust_sys_utf8_seq(p, i, len, &bad);
        if (n) { i += n; continue; }
        lust_string_push_bytes(out, p + start, i - start);
        lust_string_push_bytes(out, "\xEF\xBF\xBD", 3);
        i += bad;
        start = i;
    }
    lust_string_push_bytes(out, p + start, len - start);
}

uint8_t lust_sys_getenv(const uint8_t *k, uint64_t klen, lust_Vec *out) {
    char *key = malloc(klen + 1);
    if (!key) lust_oom();
    for (uint64_t i = 0; i < klen; i++) {
        if (k[i] == 0) { free(key); return 0; }
        key[i] = (char)k[i];
    }
    key[klen] = 0;
    const char *v = getenv(key);
    free(key);
    if (!v) return 0;
    uint64_t n = strlen(v);
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, v, n);
    out->len += n;
    return 1;
}

/* Appends the bytes of a C string and a NUL terminator. */
static void lust_sys_push_cstr0(lust_Vec *out, const char *s) {
    uint64_t n = strlen(s) + 1;
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, s, n);
    out->len += n;
}

/* A path or name as a C string: malloc'd, or NULL when it holds a NUL
   byte (std's "file name contained an unexpected NUL byte"). */
static char *lust_sys_cstr(const uint8_t *p, uint64_t len) {
    if (memchr(p, 0, len)) return NULL;
    char *s = malloc(len + 1);
    if (!s) lust_oom();
    memcpy(s, p, len);
    s[len] = 0;
    return s;
}

void lust_sys_environ(lust_Vec *out) {
    for (char **e = environ; e && *e; e++) lust_sys_push_cstr0(out, *e);
}

void lust_sys_argv(lust_Vec *out) {
    for (int i = 0; i < lust_argc; i++) lust_sys_push_cstr0(out, lust_argv[i]);
}

int32_t lust_sys_getcwd(lust_Vec *out) {
    uint64_t cap = 512;
    for (;;) {
        char *buf = malloc(cap);
        if (!buf) lust_oom();
        if (getcwd(buf, cap)) {
            uint64_t n = strlen(buf);
            lust_vec_reserve(out, n, 1, 1);
            memcpy((uint8_t *)out->ptr + out->len, buf, n);
            out->len += n;
            free(buf);
            return 0;
        }
        int e = errno;
        free(buf);
        if (e != ERANGE) return e;
        cap *= 2;
    }
}

int32_t lust_sys_chdir(const uint8_t *p, uint64_t len) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    int r = chdir(s);
    int e = errno;
    free(s);
    return r == 0 ? 0 : e;
}

int32_t lust_sys_symlink(const uint8_t *a, uint64_t alen, const uint8_t *b, uint64_t blen) {
    char *x = lust_sys_cstr(a, alen);
    if (!x) return -1;
    char *y = lust_sys_cstr(b, blen);
    if (!y) {
        free(x);
        return -1;
    }
    int r = symlink(x, y);
    int e = errno;
    free(x);
    free(y);
    return r == 0 ? 0 : e;
}

int32_t lust_sys_readlink(const uint8_t *p, uint64_t len, lust_Vec *out) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
#ifdef __APPLE__
    /* `File`'s Debug reads `/proc/self/fd/N`. Darwin has no procfs;
       `F_GETPATH` is the path rustc prints. */
    if (len >= 14 && memcmp(s, "/proc/self/fd/", 14) == 0) {
        int fd = 0;
        int ok = 1;
        for (char *c = s + 14; *c; c++) {
            if (*c < '0' || *c > '9') {
                ok = 0;
                break;
            }
            fd = fd * 10 + (*c - '0');
        }
        if (ok && s[14] != '\0') {
            char buf[1024];
            if (fcntl(fd, F_GETPATH, buf) == 0) {
                uint64_t n = strlen(buf);
                lust_vec_reserve(out, n, 1, 1);
                memcpy((uint8_t *)out->ptr + out->len, buf, (size_t)n);
                out->len += n;
                free(s);
                return 0;
            }
        }
    }
#endif
    uint64_t cap = 256;
    for (;;) {
        char *buf = malloc(cap);
        if (!buf) lust_oom();
        ssize_t n = readlink(s, buf, cap);
        if (n < 0) {
            int e = errno;
            free(buf);
            free(s);
            return e;
        }
        if ((uint64_t)n < cap) {
            lust_vec_reserve(out, (uint64_t)n, 1, 1);
            memcpy((uint8_t *)out->ptr + out->len, buf, (size_t)n);
            out->len += (uint64_t)n;
            free(buf);
            free(s);
            return 0;
        }
        free(buf);
        cap *= 2;
    }
}

/* Appends `n` bytes to a Vec<u8>. */
static void lust_sys_push(lust_Vec *out, const void *p, uint64_t n) {
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, p, (size_t)n);
    out->len += n;
}

/* As std's fs::read: the Vec gets the size fstat reports, and the bytes are read straight
   into its spare capacity. When it is full, a read into a small buffer finds the end
   without growing it (std's small_probe_read); bytes past the reported size (a file that
   grew, or one whose size reads as 0) grow the Vec as usual. */
int32_t lust_sys_read_file(const uint8_t *p, uint64_t len, lust_Vec *out) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    int fd = open(s, O_RDONLY | O_CLOEXEC);
    int e = errno;
    free(s);
    if (fd < 0) return e;
    struct stat st;
    if (fstat(fd, &st) == 0 && st.st_size > 0) lust_vec_reserve_exact(out, (uint64_t)st.st_size, 1, 1);
    for (;;) {
        uint8_t probe[32];
        uint64_t room = out->cap - out->len;
        ssize_t n = room ? read(fd, (uint8_t *)out->ptr + out->len, room > 0x7ffff000 ? 0x7ffff000 : (size_t)room)
                         : read(fd, probe, sizeof probe);
        if (n < 0) {
            if (errno == EINTR) continue;
            e = errno;
            close(fd);
            return e;
        }
        if (n == 0) break;
        if (room) out->len += (uint64_t)n;
        else lust_sys_push(out, probe, (uint64_t)n);
    }
    close(fd);
    return 0;
}

int32_t lust_sys_write_file(const uint8_t *p, uint64_t len, const uint8_t *d, uint64_t dlen) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    int fd = open(s, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0666);
    int e = errno;
    free(s);
    if (fd < 0) return e;
    while (dlen > 0) {
        ssize_t n = write(fd, d, dlen > 0x7ffff000 ? 0x7ffff000 : (size_t)dlen);
        if (n < 0) {
            if (errno == EINTR) continue;
            e = errno;
            close(fd);
            return e;
        }
        if (n == 0) {
            close(fd);
            return -2; /* WriteZero */
        }
        d += n;
        dlen -= (uint64_t)n;
    }
    close(fd);
    return 0;
}

/* One path syscall: 0, errno, or -1 for a NUL byte. `op`: 0 mkdir
   (0777), 1 unlink, 2 rmdir, 3 | mode << 8 mkdir with that mode
   (`DirBuilder`; the kernel keeps only the low 12 bits anyway). */
int32_t lust_sys_path_op(const uint8_t *p, uint64_t len, int32_t op) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    int code = op & 0xff;
    int r;
    if (code == 4) {
        /* std's `fs::set_permissions`: chmod, retried on EINTR (cvt_r).
           The kernel keeps only the S_IALLUGO bits, the ones passed. */
        do r = chmod(s, (mode_t)((op >> 8) & 07777)); while (r != 0 && errno == EINTR);
    } else {
        r = code == 0 ? mkdir(s, 0777) : code == 1 ? unlink(s) : code == 3 ? mkdir(s, (mode_t)((op >> 8) & 07777)) : rmdir(s);
    }
    int e = errno;
    free(s);
    return r == 0 ? 0 : e;
}

int32_t lust_sys_rename(const uint8_t *a, uint64_t alen, const uint8_t *b, uint64_t blen) {
    char *x = lust_sys_cstr(a, alen);
    if (!x) return -1;
    char *y = lust_sys_cstr(b, blen);
    if (!y) {
        free(x);
        return -1;
    }
    int r = rename(x, y);
    int e = errno;
    free(x);
    free(y);
    return r == 0 ? 0 : e;
}

/* The birth time behind `Metadata::created`. On Linux std reads it
   through statx with AT_STATX_SYNC_AS_STAT: 1 when the filesystem
   reports one (STATX_BTIME in the mask). The kernel's struct statx
   (stable ABI, 256 bytes) is spelled out as in sysx.c, with its own
   name. On Darwin every stat carries st_birthtimespec and std
   returns it as the created time, so one is always reported. */
#ifdef __APPLE__
int32_t lust_sys_btime_path(const char *path, int32_t follow, int64_t *sec, int64_t *nsec) {
    struct stat st;
    if ((follow ? stat(path, &st) : lstat(path, &st)) != 0) return 0;
    *sec = (int64_t)LUST_ST_BTIME(st).tv_sec;
    *nsec = (int64_t)LUST_ST_BTIME(st).tv_nsec;
    return 1;
}

int32_t lust_sys_btime_fd(int32_t fd, int64_t *sec, int64_t *nsec) {
    struct stat st;
    if (fstat(fd, &st) != 0) return 0;
    *sec = (int64_t)LUST_ST_BTIME(st).tv_sec;
    *nsec = (int64_t)LUST_ST_BTIME(st).tv_nsec;
    return 1;
}
#else
struct lust_btime_time {
    int64_t tv_sec;
    uint32_t tv_nsec;
    int32_t reserved;
};
struct lust_btime_statx {
    uint32_t stx_mask;
    uint32_t stx_blksize;
    uint64_t stx_attributes;
    uint32_t stx_nlink;
    uint32_t stx_uid;
    uint32_t stx_gid;
    uint16_t stx_mode;
    uint16_t spare0[1];
    uint64_t stx_ino;
    uint64_t stx_size;
    uint64_t stx_blocks;
    uint64_t stx_attributes_mask;
    struct lust_btime_time stx_atime;
    struct lust_btime_time stx_btime;
    struct lust_btime_time stx_ctime;
    struct lust_btime_time stx_mtime;
    uint32_t stx_rdev_major;
    uint32_t stx_rdev_minor;
    uint32_t stx_dev_major;
    uint32_t stx_dev_minor;
    uint64_t stx_mnt_id;
    uint32_t stx_dio_mem_align;
    uint32_t stx_dio_offset_align;
    uint64_t spare3[12];
};
#ifndef LUST_SYS_statx
#define LUST_SYS_statx 332
#endif

static int32_t lust_btime_read(int32_t dirfd, const char *path, int32_t flags, int64_t *sec, int64_t *nsec) {
    struct lust_btime_statx stx;
    long r = syscall(LUST_SYS_statx, dirfd, path, flags, 0x800 /* STATX_BTIME */, &stx);
    if (r != 0 || !(stx.stx_mask & 0x800)) return 0;
    *sec = stx.stx_btime.tv_sec;
    *nsec = stx.stx_btime.tv_nsec;
    return 1;
}

int32_t lust_sys_btime_path(const char *path, int32_t follow, int64_t *sec, int64_t *nsec) {
    /* AT_STATX_SYNC_AS_STAT (0x800) in the high flags is 0; the
       nofollow bit is AT_SYMLINK_NOFOLLOW (0x100). */
    return lust_btime_read(AT_FDCWD, path, follow ? 0 : 0x100, sec, nsec);
}

int32_t lust_sys_btime_fd(int32_t fd, int64_t *sec, int64_t *nsec) {
    return lust_btime_read(fd, "", 0x1000 /* AT_EMPTY_PATH */, sec, nsec);
}

#endif

/* stat (follow != 0) or lstat into 8-byte little-endian fields of the
   Vec: mode, size, mtime seconds, mtime nanoseconds, uid, gid, nlink,
   ino, dev, atime s, atime ns, ctime s, ctime ns, blocks, blksize, rdev,
   btime s, btime ns, has-btime (Metadata::created). */
int32_t lust_sys_stat(const uint8_t *p, uint64_t len, uint8_t follow, lust_Vec *out) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    struct stat st;
    int r = follow ? stat(s, &st) : lstat(s, &st);
    int e = errno;
    if (r != 0) {
        free(s);
        return e;
    }
    int64_t bsec = 0, bnsec = 0;
    int32_t has_btime = lust_sys_btime_path(s, follow, &bsec, &bnsec);
    free(s);
    uint64_t f[19] = {st.st_mode, (uint64_t)st.st_size, (uint64_t)LUST_ST_MTIME(st).tv_sec, (uint64_t)LUST_ST_MTIME(st).tv_nsec, st.st_uid, st.st_gid,
                      (uint64_t)st.st_nlink, (uint64_t)st.st_ino, (uint64_t)st.st_dev, (uint64_t)LUST_ST_ATIME(st).tv_sec,
                      (uint64_t)LUST_ST_ATIME(st).tv_nsec, (uint64_t)LUST_ST_CTIME(st).tv_sec, (uint64_t)LUST_ST_CTIME(st).tv_nsec, (uint64_t)st.st_blocks,
                      (uint64_t)st.st_blksize, (uint64_t)st.st_rdev, (uint64_t)bsec, (uint64_t)bnsec, (uint64_t)has_btime};
    for (int i = 0; i < 19; i++) {
        uint8_t b[8];
        for (int k = 0; k < 8; k++) b[k] = (uint8_t)(f[i] >> (8 * k));
        lust_sys_push(out, b, 8);
    }
    return 0;
}

int32_t lust_sys_realpath(const uint8_t *p, uint64_t len, lust_Vec *out) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    char *r = realpath(s, NULL);
    int e = errno;
    free(s);
    if (!r) return e;
    lust_sys_push(out, r, strlen(r));
    free(r);
    return 0;
}

/* The names in a directory except `.` and `..`, each with a NUL
   terminator, in readdir order. */
int32_t lust_sys_read_dir(const uint8_t *p, uint64_t len, lust_Vec *out) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    DIR *d = opendir(s);
    int e = errno;
    free(s);
    if (!d) return e;
    for (;;) {
        errno = 0;
        struct dirent *ent = readdir(d);
        if (!ent) {
            e = errno;
            closedir(d);
            return e;
        }
        const char *n = ent->d_name;
        if (n[0] == '.' && (n[1] == 0 || (n[1] == '.' && n[2] == 0))) continue;
        lust_sys_push(out, n, strlen(n) + 1);
    }
}

/* std's remove_dir_all on Linux: a symlink is unlinked; a directory is
   emptied depth first (children that vanish meanwhile are fine) and
   removed; anything else fails with ENOTDIR. */
static int lust_sys_rm_tree(int parent, const char *name) {
    int fd = openat(parent, name, O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW);
    if (fd < 0) {
        int e = errno;
        if (e == ENOTDIR || e == ELOOP) return unlinkat(parent, name, 0) == 0 || errno == ENOENT ? 0 : errno;
        return e;
    }
    DIR *d = fdopendir(fd);
    if (!d) {
        int e = errno;
        close(fd);
        return e;
    }
    for (;;) {
        errno = 0;
        struct dirent *ent = readdir(d);
        if (!ent) {
            int e = errno;
            if (e) {
                closedir(d);
                return e;
            }
            break;
        }
        const char *n = ent->d_name;
        if (n[0] == '.' && (n[1] == 0 || (n[1] == '.' && n[2] == 0))) continue;
        int e = lust_sys_rm_tree(dirfd(d), n);
        if (e && e != ENOENT) {
            closedir(d);
            return e;
        }
    }
    closedir(d);
    if (unlinkat(parent, name, AT_REMOVEDIR) != 0 && errno != ENOENT) return errno;
    return 0;
}

int32_t lust_sys_remove_dir_all(const uint8_t *p, uint64_t len) {
    char *s = lust_sys_cstr(p, len);
    if (!s) return -1;
    struct stat st;
    int e = 0;
    if (lstat(s, &st) != 0) e = errno;
    else if (S_ISLNK(st.st_mode)) e = unlink(s) == 0 ? 0 : errno;
    else {
        int fd = open(s, O_RDONLY | O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW);
        if (fd < 0) e = errno;
        else {
            close(fd);
            e = lust_sys_rm_tree(AT_FDCWD, s);
        }
    }
    free(s);
    return e;
}

uint32_t lust_sys_getpid(void) { return (uint32_t)getpid(); }

/* `env::current_exe`: std reads /proc/self/exe on Linux and
   _NSGetExecutablePath on Darwin (two calls: the size with a NULL
   buffer, then the path, its trailing NUL left off). */
#ifdef __APPLE__
int _NSGetExecutablePath(char *buf, uint32_t *bufsize);

int32_t lust_sys_current_exe(lust_Vec *out) {
    uint32_t cap = 0;
    _NSGetExecutablePath(NULL, &cap);
    if (cap == 0) return errno ? errno : EIO;
    char *buf = malloc(cap);
    if (!buf) lust_oom();
    if (_NSGetExecutablePath(buf, &cap) != 0) {
        int e = errno ? errno : EIO;
        free(buf);
        return e;
    }
    lust_sys_push(out, buf, cap - 1);
    free(buf);
    return 0;
}
#else
int32_t lust_sys_current_exe(lust_Vec *out) {
    return lust_sys_readlink((const uint8_t *)"/proc/self/exe", 14, out);
}
#endif
#endif
