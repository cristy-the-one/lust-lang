/* Bodies of the std platform intrinsics declared in sys.h, for
   Windows (rel-win). Spliced instead of sys.c under _WIN32 (see
   cgen::runtime_source). Paths cross from Rust as UTF-8 bytes (lust's
   OsStr is bytes today; full WTF-8 over UTF-16 is a later slice) and
   go to the wide Win32 calls. Errors return as GetLastError codes,
   as std's io::Error::last_os_error reads them on Windows; the text
   comes from FormatMessageW. A path with a NUL byte returns -1, as
   on Linux. */

int32_t lust_stdout_write_soft(const uint8_t *p, uint64_t n); /* panic.c */
int32_t lust_stdout_flush_soft(void);

int64_t lust_sys_write_std(int32_t fd, const uint8_t *p, uint64_t len) {
    if (len == 0) return 0;
    if (fd != 2) {
        int32_t e = lust_stdout_write_soft(p, len);
        return e ? -(int64_t)e : (int64_t)len;
    }
    HANDLE h = GetStdHandle((DWORD)-12);
    uint64_t left = len;
    const uint8_t *q = p;
    while (left) {
        DWORD done = 0;
        DWORD want = (DWORD)(left > 0x7fffffff ? 0x7fffffff : left);
        if (!WriteFile(h, q, want, &done, 0)) return -(int64_t)GetLastError();
        if (done == 0) break;
        q += done;
        left -= done;
    }
    return (int64_t)(len - left);
}

int32_t lust_sys_flush_std(int32_t fd) {
    if (fd != 2) return lust_stdout_flush_soft();
    return 0; /* stderr is unbuffered here, as in std */
}

void lust_sys_os_error_text(int32_t code, lust_String *out) {
    WCHAR buf[512];
    DWORD n = FormatMessageW(0x1000 /* FROM_SYSTEM */ | 0x200 /* IGNORE_INSERTS */, 0, (DWORD)code, 0x409, buf, 512, 0);
    if (n == 0) n = FormatMessageW(0x1000 | 0x200, 0, (DWORD)code, 0, buf, 512, 0);
    while (n > 0 && (buf[n - 1] == L'\r' || buf[n - 1] == L'\n' || buf[n - 1] == L'.')) n--;
    if (n == 0) {
        lust_string_push_cstr(out, "Unknown error");
        return;
    }
    int need = WideCharToMultiByte(65001 /* UTF-8 */, 0, buf, (int)n, 0, 0, 0, 0);
    char *u = malloc((size_t)need + 1);
    if (!u) lust_oom();
    WideCharToMultiByte(65001, 0, buf, (int)n, u, need, 0, 0);
    u[need] = 0;
    lust_string_push_cstr(out, u);
    free(u);
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

/* Appends `n` bytes to a Vec<u8>. */
static void lust_sys_push(lust_Vec *out, const void *p, uint64_t n) {
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, p, (size_t)n);
    out->len += n;
}

/* A path as a wide string: malloc'd, or NULL for a NUL byte. */
static WCHAR *lust_sys_wide(const uint8_t *p, uint64_t len) {
    if (memchr(p, 0, (size_t)len)) return NULL;
    int n = MultiByteToWideChar(65001, 0, (const char *)p, (int)len, 0, 0);
    WCHAR *w = malloc(sizeof(WCHAR) * ((size_t)n + 1));
    if (!w) lust_oom();
    MultiByteToWideChar(65001, 0, (const char *)p, (int)len, w, n);
    w[n] = 0;
    return w;
}

/* Appends a wide string as UTF-8 bytes (no terminator). */
static void lust_sys_push_wide(lust_Vec *out, const WCHAR *w, int n) {
    int need = WideCharToMultiByte(65001, 0, w, n, 0, 0, 0, 0);
    char *u = malloc((size_t)need + 1);
    if (!u) lust_oom();
    WideCharToMultiByte(65001, 0, w, n, u, need, 0, 0);
    lust_sys_push(out, u, (uint64_t)need);
    free(u);
}

static void lust_sys_push_wide0(lust_Vec *out, const WCHAR *w) {
    int n = 0;
    while (w[n]) n++;
    lust_sys_push_wide(out, w, n);
    uint8_t z = 0;
    lust_sys_push(out, &z, 1);
}

uint8_t lust_sys_getenv(const uint8_t *k, uint64_t klen, lust_Vec *out) {
    WCHAR *key = lust_sys_wide(k, klen);
    if (!key) return 0;
    DWORD n = GetEnvironmentVariableW(key, 0, 0);
    if (n == 0) { free(key); return 0; }
    WCHAR *v = malloc(sizeof(WCHAR) * n);
    if (!v) lust_oom();
    GetEnvironmentVariableW(key, v, n);
    free(key);
    lust_sys_push_wide(out, v, (int)n - 1);
    free(v);
    return 1;
}

void lust_sys_environ(lust_Vec *out) {
    WCHAR *e = GetEnvironmentStringsW();
    if (!e) return;
    for (WCHAR *p = e; *p; p++) {
        lust_sys_push_wide0(out, p);
        while (*p) p++;
    }
    FreeEnvironmentStringsW(e);
}

void lust_sys_argv(lust_Vec *out) {
    int argc = 0;
    WCHAR **argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    if (!argv) return;
    for (int i = 0; i < argc; i++) lust_sys_push_wide0(out, argv[i]);
    LocalFree(argv);
}

int32_t lust_sys_getcwd(lust_Vec *out) {
    DWORD n = GetCurrentDirectoryW(0, 0);
    if (n == 0) return (int32_t)GetLastError();
    WCHAR *buf = malloc(sizeof(WCHAR) * n);
    if (!buf) lust_oom();
    if (GetCurrentDirectoryW(n, buf) == 0) { int32_t e = (int32_t)GetLastError(); free(buf); return e; }
    lust_sys_push_wide(out, buf, (int)n - 1);
    free(buf);
    return 0;
}

int32_t lust_sys_chdir(const uint8_t *p, uint64_t len) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    int r = SetCurrentDirectoryW(w) ? 0 : (int)GetLastError();
    free(w);
    return r;
}

int32_t lust_sys_symlink(const uint8_t *a, uint64_t alen, const uint8_t *b, uint64_t blen) {
    WCHAR *x = lust_sys_wide(a, alen);
    if (!x) return -1;
    WCHAR *y = lust_sys_wide(b, blen);
    if (!y) { free(x); return -1; }
    /* 2 is SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE. The dir bit
       (1) needs the target kind; std's symlink_dir sets it. This
       intrinsic serves fs::symlink's Unix form, which std on Windows
       splits into symlink_file/dir: file form here (gap noted). */
    int r = CreateSymbolicLinkW(y, x, 2) ? 0 : (int)GetLastError();
    free(x);
    free(y);
    return r;
}

/* The final path of an open handle as UTF-8, with the \\?\ prefix
   std's canonicalize keeps (goport strips it itself). */
static int32_t lust_sys_final_path(HANDLE h, lust_Vec *out) {
    WCHAR buf[1024];
    DWORD n = GetFinalPathNameByHandleW(h, buf, 1024, 0);
    if (n == 0) return (int32_t)GetLastError();
    if (n < 1024) { lust_sys_push_wide(out, buf, (int)n); return 0; }
    WCHAR *big = malloc(sizeof(WCHAR) * (n + 1));
    if (!big) lust_oom();
    n = GetFinalPathNameByHandleW(h, big, n + 1, 0);
    if (n == 0) { int32_t e = (int32_t)GetLastError(); free(big); return e; }
    lust_sys_push_wide(out, big, (int)n);
    free(big);
    return 0;
}

int32_t lust_sys_readlink(const uint8_t *p, uint64_t len, lust_Vec *out) {
    /* env::current_exe reads /proc/self/exe through this intrinsic. */
    if (len == 14 && memcmp(p, "/proc/self/exe", 14) == 0) {
        WCHAR buf[1024];
        DWORD n = GetModuleFileNameW(0, buf, 1024);
        if (n == 0) return (int32_t)GetLastError();
        lust_sys_push_wide(out, buf, (int)n);
        return 0;
    }
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    HANDLE h = CreateFileW(w, 0, 7, 0, 3 /* OPEN_EXISTING */, 0x200000 /* BACKUP_SEMANTICS */ | 0x2000000 /* OPEN_REPARSE_POINT */, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return (int32_t)GetLastError();
    int32_t r = lust_sys_final_path(h, out);
    CloseHandle(h);
    return r;
}

int32_t lust_sys_read_file(const uint8_t *p, uint64_t len, lust_Vec *out) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    HANDLE h = CreateFileW(w, 0x80000000u /* GENERIC_READ */, 7, 0, 3, 0x200000, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return (int32_t)GetLastError();
    uint8_t buf[65536];
    for (;;) {
        DWORD n = 0;
        if (!ReadFile(h, buf, sizeof buf, &n, 0)) { int32_t e = (int32_t)GetLastError(); CloseHandle(h); return e; }
        if (n == 0) break;
        lust_sys_push(out, buf, n);
    }
    CloseHandle(h);
    return 0;
}

int32_t lust_sys_write_file(const uint8_t *p, uint64_t len, const uint8_t *d, uint64_t dlen) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    HANDLE h = CreateFileW(w, 0x40000000u /* GENERIC_WRITE */, 0, 0, 2 /* CREATE_ALWAYS */, 0x200000, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return (int32_t)GetLastError();
    while (dlen > 0) {
        DWORD n = 0;
        DWORD want = (DWORD)(dlen > 0x7fffffff ? 0x7fffffff : dlen);
        if (!WriteFile(h, d, want, &n, 0)) { int32_t e = (int32_t)GetLastError(); CloseHandle(h); return e; }
        if (n == 0) { CloseHandle(h); return -2; }
        d += n;
        dlen -= n;
    }
    CloseHandle(h);
    return 0;
}

int32_t lust_sys_path_op(const uint8_t *p, uint64_t len, int32_t op) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    int code = op & 0xff;
    int ok;
    if (code == 0 || code == 3) ok = CreateDirectoryW(w, 0);
    else if (code == 1) ok = DeleteFileW(w);
    else if (code == 4) { /* chmod: only the read-only bit exists here */
        DWORD at = GetFileAttributesW(w);
        if (at == 0xFFFFFFFFu) { int32_t e = (int32_t)GetLastError(); free(w); return e; }
        at = ((op >> 8) & 0200) ? (at & ~1u) : (at | 1u);
        ok = SetFileAttributesW(w, at);
    } else ok = RemoveDirectoryW(w);
    int32_t r = ok ? 0 : (int32_t)GetLastError();
    free(w);
    return r;
}

int32_t lust_sys_rename(const uint8_t *a, uint64_t alen, const uint8_t *b, uint64_t blen) {
    WCHAR *x = lust_sys_wide(a, alen);
    if (!x) return -1;
    WCHAR *y = lust_sys_wide(b, blen);
    if (!y) { free(x); return -1; }
    int r = MoveFileExW(x, y, 1 /* REPLACE_EXISTING */) ? 0 : (int)GetLastError();
    free(x);
    free(y);
    return r;
}

int32_t lust_sys_btime_path(const char *path, int32_t follow, int64_t *sec, int64_t *nsec) {
    (void)path; (void)follow; (void)sec; (void)nsec;
    return 0; /* filled from the stat fields below */
}

int32_t lust_sys_btime_fd(int32_t fd, int64_t *sec, int64_t *nsec) {
    (void)fd; (void)sec; (void)nsec;
    return 0;
}

/* A FILETIME to (Unix seconds, nanoseconds). */
static void lust_sys_ft(FILETIME ft, uint64_t *sec, uint64_t *nsec) {
    uint64_t t = ((uint64_t)ft.dwHighDateTime << 32) | ft.dwLowDateTime;
    if (t < 116444736000000000ull) { *sec = 0; *nsec = 0; return; }
    t -= 116444736000000000ull;
    *sec = t / 10000000ull;
    *nsec = (t % 10000000ull) * 100ull;
}

/* The 19 stat fields sys.c writes, from handle info. The mode is the
   Unix kind bits fs.rs reads (dir, regular, symlink for a reparse
   point) with 0755/0644; std on Windows has no mode, so only the
   kind and the read-only bit can match. Field 4 (uid slot) carries
   the raw file attributes for os::windows::fs::MetadataExt. */
void lust_sys_stat_fields(lust_Vec *out, DWORD attrs, uint64_t size, FILETIME ct, FILETIME at, FILETIME mt, uint64_t ino) {
    uint64_t cs, cn, as, an, ms, mn;
    lust_sys_ft(ct, &cs, &cn);
    lust_sys_ft(at, &as, &an);
    lust_sys_ft(mt, &ms, &mn);
    uint64_t mode = (attrs & 0x10) ? 0040755 : (attrs & 0x400) ? 0120777 : 0100644;
    if (attrs & 1) mode &= ~0222u;
    uint64_t f[19] = {mode, size, ms, mn, attrs, 0, 1, ino, 0, as, an, cs, cn, 0, 4096, 0, cs, cn, 1};
    for (int i = 0; i < 19; i++) {
        uint8_t b[8];
        for (int k = 0; k < 8; k++) b[k] = (uint8_t)(f[i] >> (8 * k));
        lust_sys_push(out, b, 8);
    }
}

int32_t lust_sys_stat(const uint8_t *p, uint64_t len, uint8_t follow, lust_Vec *out) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    DWORD flags = 0x200000; /* BACKUP_SEMANTICS: dirs open too */
    if (!follow) flags |= 0x2000000; /* OPEN_REPARSE_POINT */
    HANDLE h = CreateFileW(w, 0, 7, 0, 3, flags, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return (int32_t)GetLastError();
    BY_HANDLE_FILE_INFORMATION info;
    if (!GetFileInformationByHandle(h, &info)) { int32_t e = (int32_t)GetLastError(); CloseHandle(h); return e; }
    CloseHandle(h);
    uint64_t size = ((uint64_t)info.nFileSizeHigh << 32) | info.nFileSizeLow;
    uint64_t ino = ((uint64_t)info.nFileIndexHigh << 32) | info.nFileIndexLow;
    lust_sys_stat_fields(out, info.dwFileAttributes, size, info.ftCreationTime, info.ftLastAccessTime, info.ftLastWriteTime, ino);
    return 0;
}

int32_t lust_sys_realpath(const uint8_t *p, uint64_t len, lust_Vec *out) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    HANDLE h = CreateFileW(w, 0, 7, 0, 3, 0x200000, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return (int32_t)GetLastError();
    int32_t r = lust_sys_final_path(h, out);
    CloseHandle(h);
    return r;
}

int32_t lust_sys_read_dir(const uint8_t *p, uint64_t len, lust_Vec *out) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    size_t wl = 0;
    while (w[wl]) wl++;
    WCHAR *pat = malloc(sizeof(WCHAR) * (wl + 3));
    if (!pat) lust_oom();
    memcpy(pat, w, sizeof(WCHAR) * wl);
    pat[wl] = L'\\';
    pat[wl + 1] = L'*';
    pat[wl + 2] = 0;
    free(w);
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pat, &fd);
    free(pat);
    if (h == INVALID_HANDLE_VALUE) {
        DWORD e = GetLastError();
        return e == 2 /* empty dir reads as no entries on Windows */ ? (int32_t)e : (int32_t)e;
    }
    for (;;) {
        if (!(fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0))))
            lust_sys_push_wide0(out, fd.cFileName);
        if (!FindNextFileW(h, &fd)) break;
    }
    FindClose(h);
    return 0;
}

int32_t lust_sys_remove_dir_all(const uint8_t *p, uint64_t len) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -1;
    /* std removes the tree depth first; SHFileOperation is shell32.
       A simple recursive walk here: delete children, then the dir. */
    DWORD at = GetFileAttributesW(w);
    if (at == 0xFFFFFFFFu) { int32_t e = (int32_t)GetLastError(); free(w); return e; }
    if (!(at & 0x10) || (at & 0x400)) {
        int r = DeleteFileW(w) ? 0 : (int)GetLastError();
        free(w);
        return r == 0 && (at & 0x400) ? 0 : (at & 0x10) ? r : 267 /* ENOTDIR as std maps it */;
    }
    size_t wl = 0;
    while (w[wl]) wl++;
    WCHAR *pat = malloc(sizeof(WCHAR) * (wl + 3));
    if (!pat) lust_oom();
    memcpy(pat, w, sizeof(WCHAR) * wl);
    pat[wl] = L'\\';
    pat[wl + 1] = L'*';
    pat[wl + 2] = 0;
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW(pat, &fd);
    free(pat);
    if (h != INVALID_HANDLE_VALUE) {
        for (;;) {
            if (!(fd.cFileName[0] == L'.' && (fd.cFileName[1] == 0 || (fd.cFileName[1] == L'.' && fd.cFileName[2] == 0)))) {
                /* Build child path bytes (UTF-8) and recurse. */
                lust_Vec child = {0, 0, 0};
                lust_sys_push_wide(&child, w, (int)wl);
                uint8_t slash = '\\';
                lust_sys_push(&child, &slash, 1);
                lust_sys_push_wide0(&child, fd.cFileName);
                if ((fd.dwFileAttributes & 0x10) && !(fd.dwFileAttributes & 0x400))
                    lust_sys_remove_dir_all((const uint8_t *)child.ptr, child.len - 1);
                else {
                    WCHAR *cw = lust_sys_wide((const uint8_t *)child.ptr, child.len - 1);
                    if (cw) { DeleteFileW(cw); free(cw); }
                }
                free(child.ptr);
            }
            if (!FindNextFileW(h, &fd)) break;
        }
        FindClose(h);
    }
    int r = RemoveDirectoryW(w) ? 0 : (int)GetLastError();
    free(w);
    return r;
}

uint32_t lust_sys_getpid(void) { return (uint32_t)GetCurrentProcessId(); }
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
