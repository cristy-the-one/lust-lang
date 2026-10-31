/* Bodies of the fd and process intrinsics declared in sysio.h, for
   Windows (rel-win). Spliced instead of sysio.c under _WIN32.
   The Rust side speaks small integer fds; a table maps them to
   HANDLEs (0, 1, 2 are the standard handles; a spawned child's pid
   maps to its process handle the same way). A socket fd stores the
   SOCKET in that HANDLE slot and sets `socket`, because ReadFile on
   an overlapped socket is not recv and DuplicateHandle is not
   WSADuplicateSocketW. Errors return as GetLastError codes. Process
   spawn is CreateProcessW with std's argument quoting; the wait
   status word is the process exit code from GetExitCodeProcess. */

static void lust_sysio_push(lust_Vec *out, const void *p, uint64_t n) {
    if (n == 0) return;
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, p, (size_t)n);
    out->len += n;
}

typedef struct { HANDLE h; int append; int socket; int nonblock; } lust_win_fd;
static lust_win_fd *lust_win_fds;
static uint64_t lust_win_fds_len, lust_win_fds_cap;

/* Defined later in this unit, in sysnetwin.c. */
int64_t lust_win_socket_recv(uintptr_t sock, uint8_t *buf, uint32_t n);
int64_t lust_win_socket_send(uintptr_t sock, const uint8_t *buf, uint32_t n);
int32_t lust_win_socket_close(uintptr_t sock);
int32_t lust_win_socket_dup(uintptr_t sock);

static HANDLE lust_win_handle(int32_t fd) {
    if (fd == 0) return GetStdHandle((DWORD)-10);
    if (fd == 1) return GetStdHandle((DWORD)-11);
    if (fd == 2) return GetStdHandle((DWORD)-12);
    if (fd < 3 || (uint64_t)fd >= lust_win_fds_len + 3) return INVALID_HANDLE_VALUE;
    return lust_win_fds[fd - 3].h;
}

/* syswin.c's read_link of `/proc/self/fd/N` (File's Debug path). */
HANDLE lust_win_fd_os_handle(int32_t fd) {
    return lust_win_handle(fd);
}

static int lust_win_fd_is_socket(int32_t fd) {
    return fd >= 3 && (uint64_t)fd < lust_win_fds_len + 3 && lust_win_fds[fd - 3].socket;
}

static void lust_win_fd_set_nonblock(int32_t fd, int on) {
    if (fd >= 3 && (uint64_t)fd < lust_win_fds_len + 3) lust_win_fds[fd - 3].nonblock = on ? 1 : 0;
}

static int lust_win_fd_nonblock(int32_t fd) {
    if (fd < 3 || (uint64_t)fd >= lust_win_fds_len + 3) return 0;
    return lust_win_fds[fd - 3].nonblock;
}

static int32_t lust_win_fd_insert(HANDLE h, int append, int socket) {
    for (uint64_t i = 0; i < lust_win_fds_len; i++) {
        if (lust_win_fds[i].h == INVALID_HANDLE_VALUE) {
            lust_win_fds[i].h = h;
            lust_win_fds[i].append = append;
            lust_win_fds[i].socket = socket;
            lust_win_fds[i].nonblock = 0;
            return (int32_t)(i + 3);
        }
    }
    if (lust_win_fds_len == lust_win_fds_cap) {
        lust_win_fds_cap = lust_win_fds_cap ? lust_win_fds_cap * 2 : 64;
        lust_win_fds = realloc(lust_win_fds, (size_t)lust_win_fds_cap * sizeof *lust_win_fds);
        if (!lust_win_fds) lust_oom();
    }
    lust_win_fds[lust_win_fds_len].h = h;
    lust_win_fds[lust_win_fds_len].append = append;
    lust_win_fds[lust_win_fds_len].socket = socket;
    lust_win_fds[lust_win_fds_len].nonblock = 0;
    return (int32_t)(lust_win_fds_len++ + 3);
}

static int32_t lust_win_fd_new(HANDLE h, int append) {
    return lust_win_fd_insert(h, append, 0);
}

static int32_t lust_win_fd_new_socket(uintptr_t sock) {
    return lust_win_fd_insert((HANDLE)sock, 0, 1);
}

int64_t lust_sys_fd_read(int32_t fd, uint64_t max, lust_Vec *out) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6; /* ERROR_INVALID_HANDLE */
    if (max == 0) return 0;
    uint32_t want = (uint32_t)(max > 0x7fffffff ? 0x7fffffff : max);
    uint8_t *tmp = malloc((size_t)want);
    if (!tmp) lust_oom();
    /* A shut-down socket reads as EOF, the same as a Unix fd. ReadFile
       on an overlapped SOCKET does not. */
    if (lust_win_fd_is_socket(fd)) {
        int64_t n = lust_win_socket_recv((uintptr_t)h, tmp, want);
        if (n > 0) lust_sysio_push(out, tmp, (uint64_t)n);
        free(tmp);
        return n;
    }
    DWORD n = 0;
    if (!ReadFile(h, tmp, (DWORD)want, &n, 0)) {
        DWORD e = GetLastError();
        free(tmp);
        if (e == 109 /* ERROR_BROKEN_PIPE: EOF on a pipe */) return 0;
        return -(int64_t)e;
    }
    lust_sysio_push(out, tmp, n);
    free(tmp);
    return (int64_t)n;
}

int64_t lust_sys_fd_write(int32_t fd, const uint8_t *p, uint64_t len) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6;
    if (lust_win_fd_is_socket(fd)) {
        if (len == 0) return 0;
        uint32_t want = (uint32_t)(len > 0x7fffffff ? 0x7fffffff : len);
        return lust_win_socket_send((uintptr_t)h, p, want);
    }
    if (fd >= 3 && lust_win_fds[fd - 3].append && GetFileType(h) == 1 /* DISK */) {
        LARGE_INTEGER z = {0};
        SetFilePointerEx(h, z, 0, 2 /* FILE_END */);
    }
    DWORD n = 0;
    DWORD want = (DWORD)(len > 0x7fffffff ? 0x7fffffff : len);
    if (!WriteFile(h, p, want, &n, 0)) return -(int64_t)GetLastError();
    return (int64_t)n;
}

int32_t lust_sys_fd_close(int32_t fd) {
    if (fd < 3) return 0;
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return 6;
    int socket = lust_win_fds[fd - 3].socket;
    lust_win_fds[fd - 3].h = INVALID_HANDLE_VALUE;
    lust_win_fds[fd - 3].socket = 0;
    lust_win_fds[fd - 3].append = 0;
    lust_win_fds[fd - 3].nonblock = 0;
    if (socket) return lust_win_socket_close((uintptr_t)h);
    return CloseHandle(h) ? 0 : (int32_t)GetLastError();
}

int32_t lust_sys_fd_dup(int32_t fd) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -6;
    if (lust_win_fd_is_socket(fd)) return lust_win_socket_dup((uintptr_t)h);
    HANDLE copy = INVALID_HANDLE_VALUE;
    if (!DuplicateHandle(GetCurrentProcess(), h, GetCurrentProcess(), &copy, 0, 0, 2 /* SAME_ACCESS */))
        return -(int32_t)GetLastError();
    return lust_win_fd_new(copy, fd >= 3 ? lust_win_fds[fd - 3].append : 0);
}

int32_t lust_sys_fd_sync(int32_t fd) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return 6;
    return FlushFileBuffers(h) ? 0 : (int32_t)GetLastError();
}

int32_t lust_sys_fd_sync_data(int32_t fd) { return lust_sys_fd_sync(fd); }

int32_t lust_sys_fd_truncate(int32_t fd, uint64_t len) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return 6;
    LARGE_INTEGER at, save;
    at.QuadPart = (int64_t)len;
    if (!SetFilePointerEx(h, at, &save, 0)) return (int32_t)GetLastError();
    /* save holds the target; move there, set EOF, move back */
    LARGE_INTEGER zero = {0}, cur;
    if (!SetFilePointerEx(h, zero, &cur, 1 /* CURRENT */)) return (int32_t)GetLastError();
    if (!SetFilePointerEx(h, at, 0, 0)) return (int32_t)GetLastError();
    if (!SetEndOfFile(h)) { int32_t e = (int32_t)GetLastError(); SetFilePointerEx(h, cur, 0, 0); return e; }
    SetFilePointerEx(h, cur, 0, 0);
    return 0;
}

int64_t lust_sys_fd_seek(int32_t fd, int64_t off, int32_t whence) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6;
    LARGE_INTEGER at, out;
    at.QuadPart = off;
    if (!SetFilePointerEx(h, at, &out, (DWORD)whence)) return -(int64_t)GetLastError();
    return (int64_t)out.QuadPart;
}

int64_t lust_sys_fd_pread(int32_t fd, uint64_t max, int64_t off, lust_Vec *out) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6;
    uint8_t *tmp = max ? malloc((size_t)max) : malloc(1);
    if (!tmp) lust_oom();
    OVERLAPPED ov = {0};
    ov.Offset = (DWORD)(uint64_t)off;
    ov.OffsetHigh = (DWORD)((uint64_t)off >> 32);
    DWORD n = 0;
    DWORD want = (DWORD)(max > 0x7fffffff ? 0x7fffffff : max);
    if (!ReadFile(h, tmp, want, &n, &ov)) {
        DWORD e = GetLastError();
        free(tmp);
        if (e == 38 /* ERROR_HANDLE_EOF */) return 0;
        return -(int64_t)e;
    }
    lust_sysio_push(out, tmp, n);
    free(tmp);
    return (int64_t)n;
}

int64_t lust_sys_fd_pwrite(int32_t fd, const uint8_t *p, uint64_t len, int64_t off) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6;
    OVERLAPPED ov = {0};
    ov.Offset = (DWORD)(uint64_t)off;
    ov.OffsetHigh = (DWORD)((uint64_t)off >> 32);
    DWORD n = 0;
    DWORD want = (DWORD)(len > 0x7fffffff ? 0x7fffffff : len);
    if (!WriteFile(h, p, want, &n, &ov)) return -(int64_t)GetLastError();
    return (int64_t)n;
}

int32_t lust_sys_fd_isatty(int32_t fd) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return 0;
    DWORD mode;
    return GetConsoleMode(h, &mode) ? 1 : 0;
}

void lust_sys_stat_fields(lust_Vec *out, DWORD attrs, uint64_t size, FILETIME ct, FILETIME at, FILETIME mt, uint64_t ino); /* syswin.c */

int32_t lust_sys_fd_stat(int32_t fd, lust_Vec *out) {
    HANDLE h = lust_win_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return 6;
    BY_HANDLE_FILE_INFORMATION info;
    if (!GetFileInformationByHandle(h, &info)) return (int32_t)GetLastError();
    uint64_t size = ((uint64_t)info.nFileSizeHigh << 32) | info.nFileSizeLow;
    uint64_t ino = ((uint64_t)info.nFileIndexHigh << 32) | info.nFileIndexLow;
    /* The field writer lives in syswin.c; declared there as static,
       so the fields are written again here through it. */
    lust_sys_stat_fields(out, info.dwFileAttributes, size, info.ftCreationTime, info.ftLastAccessTime, info.ftLastWriteTime, ino);
    return 0;
}

/* flags as in sysio.c: bits 0-1 access, bit 2 create, bit 3 trunc,
   bit 4 append, bit 5 excl. */
static int32_t lust_win_open(const uint8_t *p, uint64_t len, int32_t flags) {
    WCHAR *w = lust_sys_wide(p, len);
    if (!w) return -87; /* ERROR_INVALID_PARAMETER for a NUL path */
    DWORD access = (flags & 3) == 0 ? 0x80000000u : (flags & 3) == 1 ? 0x40000000u : (0x80000000u | 0x40000000u);
    DWORD disp;
    if ((flags & 4) && (flags & 32)) disp = 1; /* CREATE_NEW */
    else if ((flags & 4) && (flags & 8)) disp = 2; /* CREATE_ALWAYS */
    else if (flags & 4) disp = 4; /* OPEN_ALWAYS */
    else if (flags & 8) disp = 5; /* TRUNCATE_EXISTING */
    else disp = 3; /* OPEN_EXISTING */
    HANDLE h = CreateFileW(w, access, 7, 0, disp, 0x02000000u /* FILE_FLAG_BACKUP_SEMANTICS */, 0);
    free(w);
    if (h == INVALID_HANDLE_VALUE) return -(int32_t)GetLastError();
    return lust_win_fd_new(h, (flags & 16) != 0);
}

int32_t lust_sys_fd_open(const uint8_t *p, uint64_t len, int32_t flags) {
    return lust_win_open(p, len, flags);
}

int32_t lust_sys_fd_open_ext(const uint8_t *p, uint64_t len, int32_t flags, int32_t custom, uint32_t mode) {
    (void)custom; (void)mode; /* Unix open flags and mode: no Windows form */
    return lust_win_open(p, len, flags);
}

static uint8_t lust_sysio_stdin_buf[8192];
static uint64_t lust_sysio_stdin_pos, lust_sysio_stdin_cap;

int64_t lust_sys_stdin_read(uint64_t max, lust_Vec *out) {
    if (max == 0) return 0;
    if (lust_sysio_stdin_pos < lust_sysio_stdin_cap) {
        uint64_t avail = lust_sysio_stdin_cap - lust_sysio_stdin_pos;
        uint64_t n = avail < max ? avail : max;
        lust_sysio_push(out, lust_sysio_stdin_buf + lust_sysio_stdin_pos, n);
        lust_sysio_stdin_pos += n;
        return (int64_t)n;
    }
    lust_sysio_stdin_pos = lust_sysio_stdin_cap = 0;
    return lust_sys_fd_read(0, max, out);
}

int64_t lust_sys_stdin_fill(lust_Vec *out) {
    if (lust_sysio_stdin_pos >= lust_sysio_stdin_cap) {
        int64_t n = lust_sys_fd_read(0, sizeof lust_sysio_stdin_buf, out);
        if (n <= 0) return n;
        /* Bytes went to out, not the buffer: report them as filled
           and mark them consumed by the caller's copy (out already
           holds them; consume() then has nothing to drop). */
        return n;
    }
    uint64_t avail = lust_sysio_stdin_cap - lust_sysio_stdin_pos;
    lust_sysio_push(out, lust_sysio_stdin_buf + lust_sysio_stdin_pos, avail);
    return (int64_t)avail;
}

void lust_sys_stdin_consume(uint64_t n) {
    uint64_t avail = lust_sysio_stdin_cap - lust_sysio_stdin_pos;
    lust_sysio_stdin_pos += n < avail ? n : avail;
}

int32_t lust_sys_fd_read_both(int32_t fd1, lust_Vec *out1, int32_t fd2, lust_Vec *out2) {
    /* No poll on pipes without overlapped io: read one to EOF, then
       the other. Command::output can deadlock when the child fills
       the second pipe while this reads the first (gap: needs two
       reader threads, next slice). */
    int32_t err = 0;
    int32_t fds[2] = {fd1, fd2};
    lust_Vec *outs[2] = {out1, out2};
    for (int i = 0; i < 2; i++) {
        if (fds[i] < 0) continue;
        for (;;) {
            int64_t n = lust_sys_fd_read(fds[i], 65536, outs[i]);
            if (n < 0) { if (!err) err = (int32_t)-n; break; }
            if (n == 0) break;
        }
    }
    return err;
}

/* Spawned children by pid: the Rust side waits and kills by pid. */
typedef struct { uint32_t pid; HANDLE h; } lust_win_child;
static lust_win_child *lust_win_children;
static uint64_t lust_win_children_len, lust_win_children_cap;

static HANDLE lust_win_child_handle(uint32_t pid) {
    for (uint64_t i = 0; i < lust_win_children_len; i++)
        if (lust_win_children[i].pid == pid) return lust_win_children[i].h;
    return INVALID_HANDLE_VALUE;
}

int64_t lust_sys_proc_wait(uint32_t pid, uint8_t block) {
    HANDLE h = lust_win_child_handle(pid);
    if (h == INVALID_HANDLE_VALUE) return -(int64_t)6;
    DWORD r = WaitForSingleObject(h, block ? 0xFFFFFFFFu : 0);
    if (r == 0x102 /* WAIT_TIMEOUT */) return -2;
    if (r != 0) return -(int64_t)GetLastError();
    DWORD code = 0;
    GetExitCodeProcess(h, &code);
    /* The process exit code itself. A Unix wait status only has 8 bits
       for it, so `kill -TERM` under Git bash (exit 3840) came back as 0. */
    return (int64_t)code;
}

int32_t lust_sys_proc_kill(uint32_t pid) {
    HANDLE h = lust_win_child_handle(pid);
    if (h == INVALID_HANDLE_VALUE) return 6;
    return TerminateProcess(h, 1) ? 0 : (int32_t)GetLastError();
}

static void lust_sysio_push_u64(lust_Vec *out, uint64_t v) {
    uint8_t b[8];
    for (int k = 0; k < 8; k++) b[k] = (uint8_t)(v >> (8 * k));
    lust_sysio_push(out, b, 8);
}

/* std's Windows argument quoting: quote when empty or holding a
   space, tab or quote; backslashes double before a quote. */
static void lust_win_quote_arg(lust_Vec *cmd, const WCHAR *a, int n) {
    int quote = n == 0;
    for (int i = 0; i < n; i++) if (a[i] == L' ' || a[i] == L'\t' || a[i] == L'"') quote = 1;
    WCHAR q = L'"', bs = L'\\';
    if (quote) lust_sysio_push(cmd, &q, 2);
    int run = 0;
    for (int i = 0; i < n; i++) {
        if (a[i] == L'\\') { run++; continue; }
        if (a[i] == L'"') {
            for (int k = 0; k < run * 2 + 1; k++) lust_sysio_push(cmd, &bs, 2);
            run = 0;
        } else {
            for (int k = 0; k < run; k++) lust_sysio_push(cmd, &bs, 2);
            run = 0;
        }
        lust_sysio_push(cmd, &a[i], 2);
    }
    if (quote) {
        for (int k = 0; k < run * 2; k++) lust_sysio_push(cmd, &bs, 2);
        lust_sysio_push(cmd, &q, 2);
    } else {
        for (int k = 0; k < run; k++) lust_sysio_push(cmd, &bs, 2);
    }
}

/* Rust's `resolve_exe`: a bare name is looked up as `name.exe` in the
   executable's directory, System32, the Windows directory, then PATH.
   A path is used as given, after trying a `.exe` suffix when it has none. */
static int lust_win_exists(const WCHAR *path) {
    return GetFileAttributesW(path) != 0xFFFFFFFFu;
}
static WCHAR *lust_win_join3(const WCHAR *dir, const WCHAR *name, const WCHAR *ext) {
    size_t nd = 0, nn = 0, ne = 0;
    while (dir[nd]) nd++;
    while (name[nn]) nn++;
    while (ext[ne]) ne++;
    int sep = nd && dir[nd - 1] != L'\\' && dir[nd - 1] != L'/';
    WCHAR *o = malloc(sizeof(WCHAR) * (nd + sep + nn + ne + 1));
    if (!o) lust_oom();
    size_t k = 0;
    for (size_t i = 0; i < nd; i++) o[k++] = dir[i];
    if (sep) o[k++] = L'\\';
    for (size_t i = 0; i < nn; i++) o[k++] = name[i];
    for (size_t i = 0; i < ne; i++) o[k++] = ext[i];
    o[k] = 0;
    return o;
}
static WCHAR *lust_win_dup(const WCHAR *s) {
    size_t n = 0;
    while (s[n]) n++;
    WCHAR *o = malloc(sizeof(WCHAR) * (n + 1));
    if (!o) lust_oom();
    for (size_t i = 0; i <= n; i++) o[i] = s[i];
    return o;
}
static int lust_win_ends_exe(const WCHAR *s) {
    size_t n = 0;
    while (s[n]) n++;
    if (n < 4) return 0;
    const WCHAR *e = s + n - 4;
    return e[0] == L'.' && (e[1] == L'e' || e[1] == L'E') && (e[2] == L'x' || e[2] == L'X') && (e[3] == L'e' || e[3] == L'E');
}
static int lust_win_has_dot(const WCHAR *s) {
    for (; *s; s++) if (*s == L'.') return 1;
    return 0;
}
static int lust_win_is_bare(const WCHAR *s) {
    for (; *s; s++) if (*s == L'\\' || *s == L'/' || *s == L':') return 0;
    return 1;
}
static WCHAR *lust_win_try_dir(const WCHAR *dir, const WCHAR *name, int add_exe) {
    const WCHAR *ext = add_exe ? L".exe" : L"";
    WCHAR *p = lust_win_join3(dir, name, ext);
    if (lust_win_exists(p)) return p;
    free(p);
    return 0;
}
static WCHAR *lust_win_resolve_prog(const WCHAR *prog) {
    if (!lust_win_is_bare(prog)) {
        if (lust_win_ends_exe(prog)) return lust_win_dup(prog);
        WCHAR *with = lust_win_join3(L"", prog, L".exe");
        if (lust_win_exists(with)) return with;
        free(with);
        return lust_win_dup(prog);
    }
    int add_exe = !lust_win_has_dot(prog);
    WCHAR exe[1024];
    DWORD n = GetModuleFileNameW(0, exe, 1024);
    if (n && n < 1024) {
        WCHAR *slash = exe;
        for (WCHAR *p = exe; *p; p++) if (*p == L'\\' || *p == L'/') slash = p;
        *slash = 0;
        WCHAR *hit = lust_win_try_dir(exe, prog, add_exe);
        if (hit) return hit;
    }
    WCHAR sys[512];
    n = GetSystemDirectoryW(sys, 512);
    if (n && n < 512) {
        WCHAR *hit = lust_win_try_dir(sys, prog, add_exe);
        if (hit) return hit;
    }
    n = GetWindowsDirectoryW(sys, 512);
    if (n && n < 512) {
        WCHAR *hit = lust_win_try_dir(sys, prog, add_exe);
        if (hit) return hit;
    }
    DWORD need = GetEnvironmentVariableW(L"PATH", 0, 0);
    if (need) {
        WCHAR *path = malloc(sizeof(WCHAR) * need);
        if (!path) lust_oom();
        if (GetEnvironmentVariableW(L"PATH", path, need)) {
            WCHAR *cur = path;
            while (*cur) {
                WCHAR *semi = cur;
                while (*semi && *semi != L';') semi++;
                WCHAR saved = *semi;
                *semi = 0;
                if (cur[0]) {
                    WCHAR *hit = lust_win_try_dir(cur, prog, add_exe);
                    if (hit) { free(path); return hit; }
                }
                if (!saved) break;
                cur = semi + 1;
            }
        }
        free(path);
    }
    return 0;
}

int32_t lust_sys_proc_spawn(const uint8_t *prog, uint64_t proglen, const uint8_t *args, uint64_t argslen,
                            const uint8_t *env, uint64_t envlen, const uint8_t *cwd, uint64_t cwdlen, uint32_t stdio,
                            const uint8_t *fds, uint64_t fdslen, lust_Vec *out) {
    if (argslen == 0 || envlen == 0) return 87;
    /* Command line: the program, then each packed arg, quoted. */
    lust_Vec cmd = {0, 0, 0};
    WCHAR *given = lust_sys_wide(prog, proglen);
    if (!given) return -1;
    WCHAR *pw = lust_win_resolve_prog(given);
    free(given);
    if (!pw) return 2; /* ERROR_FILE_NOT_FOUND */
    int pn = 0;
    while (pw[pn]) pn++;
    lust_win_quote_arg(&cmd, pw, pn);
    uint64_t start = 0;
    int first = 1;
    for (uint64_t i = 0; i < argslen; i++) {
        if (args[i] != 0) continue;
        if (!first) {
            WCHAR sp = L' ';
            lust_sysio_push(&cmd, &sp, 2);
            WCHAR *aw = lust_sys_wide(args + start, i - start);
            if (!aw) { free(pw); free(cmd.ptr); return -1; }
            int an = 0;
            while (aw[an]) an++;
            lust_win_quote_arg(&cmd, aw, an);
            free(aw);
        }
        first = 0;
        start = i + 1;
    }
    WCHAR z = 0;
    lust_sysio_push(&cmd, &z, 2);
    /* Environment: packed K=V strings after a flag byte, or inherit. */
    void *envblock = 0;
    lust_Vec envv = {0, 0, 0};
    if (env[0] != 0) {
        uint64_t s0 = 1;
        for (uint64_t i = 1; i < envlen; i++) {
            if (env[i] != 0) continue;
            WCHAR *ew = lust_sys_wide(env + s0, i - s0);
            if (!ew) { free(pw); free(cmd.ptr); return -1; }
            int en = 0;
            while (ew[en]) en++;
            lust_sysio_push(&envv, ew, (uint64_t)(en + 1) * 2);
            free(ew);
            s0 = i + 1;
        }
        lust_sysio_push(&envv, &z, 2);
        envblock = envv.ptr;
    }
    WCHAR *cwdw = 0;
    if ((stdio >> 6) & 1) {
        cwdw = lust_sys_wide(cwd, cwdlen);
        if (!cwdw) { free(pw); free(cmd.ptr); free(envv.ptr); return -1; }
    }
    int spec[3] = {(int)(stdio & 3), (int)((stdio >> 2) & 3), (int)((stdio >> 4) & 3)};
    HANDLE child_io[3];
    int32_t parent_fd[3] = {-1, -1, -1};
    HANDLE close_after[3] = {0, 0, 0};
    SECURITY_ATTRIBUTES sa = {sizeof sa, 0, 1}; /* inheritable */
    for (int i = 0; i < 3; i++) {
        if (spec[i] == 0) {
            child_io[i] = GetStdHandle(i == 0 ? (DWORD)-10 : i == 1 ? (DWORD)-11 : (DWORD)-12);
        } else if (spec[i] == 1) {
            child_io[i] = CreateFileW(L"NUL", i == 0 ? 0x80000000u : 0x40000000u, 7, &sa, 3, 0, 0);
            close_after[i] = child_io[i];
        } else if (spec[i] == 2) {
            HANDLE rd = INVALID_HANDLE_VALUE, wr = INVALID_HANDLE_VALUE;
            CreatePipe(&rd, &wr, &sa, 0);
            if (i == 0) { child_io[i] = rd; parent_fd[i] = lust_win_fd_new(wr, 0); SetHandleInformation(wr, 1, 0); }
            else { child_io[i] = wr; parent_fd[i] = lust_win_fd_new(rd, 0); SetHandleInformation(rd, 1, 0); }
            close_after[i] = child_io[i];
        } else {
            uint64_t v = 0;
            if (fdslen >= (uint64_t)(i + 1) * 8)
                for (int k = 0; k < 8; k++) v |= (uint64_t)fds[i * 8 + k] << (8 * k);
            HANDLE src = lust_win_handle((int32_t)(int64_t)v);
            HANDLE dup = INVALID_HANDLE_VALUE;
            DuplicateHandle(GetCurrentProcess(), src, GetCurrentProcess(), &dup, 0, 1, 2);
            child_io[i] = dup;
            close_after[i] = dup;
        }
    }
    STARTUPINFOW si = {0};
    si.cb = sizeof si;
    si.dwFlags = 0x100; /* STARTF_USESTDHANDLES */
    si.hStdInput = child_io[0];
    si.hStdOutput = child_io[1];
    si.hStdError = child_io[2];
    PROCESS_INFORMATION pi = {0};
    int ok = CreateProcessW(pw, (WCHAR *)cmd.ptr, 0, 0, 1, 0x400 /* CREATE_UNICODE_ENVIRONMENT */, envblock, cwdw, &si, &pi);
    int32_t err = ok ? 0 : (int32_t)GetLastError();
    for (int i = 0; i < 3; i++) if (close_after[i] && close_after[i] != INVALID_HANDLE_VALUE) CloseHandle(close_after[i]);
    free(pw);
    free(cmd.ptr);
    free(envv.ptr);
    free(cwdw);
    if (!ok) return err;
    CloseHandle(pi.hThread);
    if (lust_win_children_len == lust_win_children_cap) {
        lust_win_children_cap = lust_win_children_cap ? lust_win_children_cap * 2 : 32;
        lust_win_children = realloc(lust_win_children, (size_t)lust_win_children_cap * sizeof *lust_win_children);
        if (!lust_win_children) lust_oom();
    }
    lust_win_children[lust_win_children_len].pid = (uint32_t)pi.dwProcessId;
    lust_win_children[lust_win_children_len].h = pi.hProcess;
    lust_win_children_len++;
    lust_sysio_push_u64(out, (uint64_t)pi.dwProcessId);
    for (int i = 0; i < 3; i++) lust_sysio_push_u64(out, (uint64_t)(int64_t)parent_fd[i]);
    return 0;
}

int32_t lust_sys_proc_exec(const uint8_t *prog, uint64_t proglen, const uint8_t *args, uint64_t argslen,
                           const uint8_t *env, uint64_t envlen, const uint8_t *cwd, uint64_t cwdlen, uint32_t stdio,
                           const uint8_t *fds, uint64_t fdslen) {
    (void)prog; (void)proglen; (void)args; (void)argslen; (void)env; (void)envlen;
    (void)cwd; (void)cwdlen; (void)stdio; (void)fds; (void)fdslen;
    return 50; /* ERROR_NOT_SUPPORTED: exec has no Windows form */
}
