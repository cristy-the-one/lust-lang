/* Socket intrinsics for Windows. Spliced instead of the Unix bodies
   in sysnet.c. The Rust side still speaks small integer fds; a socket
   lives in the fd table (sysiowin.c) with its SOCKET in the HANDLE
   slot. TCP and UDP follow rustc 1.99's Windows std: WSASocketW with
   the no-inherit flag, no SO_REUSEADDR on bind, millisecond socket
   timeouts, and WSAGetLastError for lookup failures. Unix-domain
   sockets stay unsupported. */
#ifdef interface
#undef interface
#endif
/* Skip the header's static inline helpers. Nothing here calls them,
   and their bodies are not what this runtime needs from the header. */
#define __CRT__NO_INLINE
#include <winsock2.h>
#include <ws2tcpip.h>

#ifndef WSA_FLAG_NO_HANDLE_INHERIT
#define WSA_FLAG_NO_HANDLE_INHERIT 0x80
#endif

static LONG lust_wsa_state;

static int lust_wsa_startup(void) {
    LONG s = InterlockedCompareExchange(&lust_wsa_state, 1, 0);
    if (s == 2) return 0;
    if (s == 0) {
        WSADATA w;
        memset(&w, 0, sizeof w);
        int rc = WSAStartup(MAKEWORD(2, 2), &w);
        if (rc != 0) {
            InterlockedExchange(&lust_wsa_state, 0);
            return rc;
        }
        InterlockedExchange(&lust_wsa_state, 2);
        return 0;
    }
    for (;;) {
        if (InterlockedCompareExchange(&lust_wsa_state, 2, 2) == 2) return 0;
        if (InterlockedCompareExchange(&lust_wsa_state, 0, 0) == 0) return lust_wsa_startup();
    }
}

static int lust_wsa_last(void) {
    int e = WSAGetLastError();
    return e ? e : WSAEINVAL;
}

static void lust_sysnet_push(lust_Vec *out, const void *p, uint64_t n) {
    if (n == 0) return;
    lust_vec_reserve(out, n, 1, 1);
    memcpy((uint8_t *)out->ptr + out->len, p, (size_t)n);
    out->len += n;
}

static void lust_sysnet_put64(lust_Vec *out, uint64_t v) {
    uint8_t b[8];
    for (int i = 0; i < 8; i++) b[i] = (uint8_t)(v >> (8 * i));
    lust_sysnet_push(out, b, 8);
}

static uint64_t lust_sysnet_get64(const uint8_t *p) {
    uint64_t v = 0;
    for (int i = 0; i < 8; i++) v |= (uint64_t)p[i] << (8 * i);
    return v;
}

static void lust_sysnet_push_addr(lust_Vec *out, const struct sockaddr *sa) {
    uint8_t bytes[16];
    memset(bytes, 0, sizeof bytes);
    if (sa->sa_family == AF_INET) {
        const struct sockaddr_in *s = (const struct sockaddr_in *)sa;
        memcpy(bytes, &s->sin_addr, 4);
        lust_sysnet_put64(out, 4);
        lust_sysnet_put64(out, ntohs(s->sin_port));
        lust_sysnet_put64(out, lust_sysnet_get64(bytes));
        lust_sysnet_put64(out, lust_sysnet_get64(bytes + 8));
        lust_sysnet_put64(out, 0);
        lust_sysnet_put64(out, 0);
    } else if (sa->sa_family == AF_INET6) {
        const struct sockaddr_in6 *s = (const struct sockaddr_in6 *)sa;
        memcpy(bytes, &s->sin6_addr, 16);
        lust_sysnet_put64(out, 6);
        lust_sysnet_put64(out, ntohs(s->sin6_port));
        lust_sysnet_put64(out, lust_sysnet_get64(bytes));
        lust_sysnet_put64(out, lust_sysnet_get64(bytes + 8));
        lust_sysnet_put64(out, s->sin6_flowinfo);
        lust_sysnet_put64(out, s->sin6_scope_id);
    }
}

static int lust_sysnet_read_addr(const uint8_t *rec, uint64_t len, struct sockaddr_storage *ss) {
    if (len < 48) return 0;
    uint64_t fam = lust_sysnet_get64(rec);
    uint16_t port = (uint16_t)lust_sysnet_get64(rec + 8);
    uint8_t bytes[16];
    for (int i = 0; i < 8; i++) bytes[i] = rec[16 + i];
    for (int i = 0; i < 8; i++) bytes[8 + i] = rec[24 + i];
    memset(ss, 0, sizeof *ss);
    if (fam == 4) {
        struct sockaddr_in *s = (struct sockaddr_in *)ss;
        s->sin_family = AF_INET;
        s->sin_port = htons(port);
        memcpy(&s->sin_addr, bytes, 4);
        return (int)sizeof *s;
    }
    if (fam == 6) {
        struct sockaddr_in6 *s = (struct sockaddr_in6 *)ss;
        s->sin6_family = AF_INET6;
        s->sin6_port = htons(port);
        memcpy(&s->sin6_addr, bytes, 16);
        s->sin6_flowinfo = (u_long)lust_sysnet_get64(rec + 32);
        s->sin6_scope_id = (u_long)lust_sysnet_get64(rec + 40);
        return (int)sizeof *s;
    }
    return 0;
}

static int lust_sysnet_family(const uint8_t *rec, uint64_t len) {
    if (len < 48) return -1;
    uint64_t fam = lust_sysnet_get64(rec);
    return fam == 4 ? AF_INET : fam == 6 ? AF_INET6 : -1;
}

/* 0, or a positive WSA error. The socket is closed on failure. */
static int lust_wsa_open(SOCKET *out, int fam, int ty) {
    int e = lust_wsa_startup();
    if (e) return e;
    DWORD flags = WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT;
    SOCKET s = WSASocketW(fam, ty, 0, 0, 0, flags);
    if (s == INVALID_SOCKET) {
        e = lust_wsa_last();
        if (e != WSAEPROTOTYPE && e != WSAEINVAL) return e;
        s = WSASocketW(fam, ty, 0, 0, 0, WSA_FLAG_OVERLAPPED);
        if (s == INVALID_SOCKET) return lust_wsa_last();
        if (!SetHandleInformation((HANDLE)(uintptr_t)s, 1, 0)) {
            e = (int)GetLastError();
            closesocket(s);
            return e ? e : WSAEINVAL;
        }
    }
    *out = s;
    return 0;
}

static int lust_take_socket(int32_t fd, SOCKET *out) {
    if (!lust_win_fd_is_socket(fd)) return 6;
    SOCKET s = (SOCKET)(uintptr_t)lust_win_handle(fd);
    if (s == INVALID_SOCKET) return 6;
    *out = s;
    return 0;
}

static void lust_push_local(SOCKET s, lust_Vec *out) {
    struct sockaddr_storage local;
    int ll = (int)sizeof local;
    if (getsockname(s, (struct sockaddr *)&local, &ll) == 0) lust_sysnet_push_addr(out, (struct sockaddr *)&local);
}

static int lust_set_nonblock(SOCKET s, int on) {
    u_long v = on ? 1u : 0u;
    return ioctlsocket(s, FIONBIO, &v) == 0 ? 0 : lust_wsa_last();
}

static DWORD lust_ms_from_us(int64_t us) {
    if (us <= 0) return 0;
    uint64_t ms = ((uint64_t)us + 999ull) / 1000ull;
    if (ms > 0xffffffffull) return 0xffffffffu;
    return (DWORD)ms;
}

int64_t lust_win_socket_recv(uintptr_t sock, uint8_t *buf, uint32_t n) {
    int r = recv((SOCKET)sock, (char *)buf, (int)n, 0);
    if (r == SOCKET_ERROR) {
        int e = WSAGetLastError();
        if (e == WSAESHUTDOWN) return 0;
        return -(int64_t)(e ? e : WSAEINVAL);
    }
    return r;
}

int64_t lust_win_socket_send(uintptr_t sock, const uint8_t *buf, uint32_t n) {
    int r = send((SOCKET)sock, (const char *)buf, (int)n, 0);
    if (r == SOCKET_ERROR) return -(int64_t)lust_wsa_last();
    return r;
}

int32_t lust_win_socket_close(uintptr_t sock) {
    if (closesocket((SOCKET)sock) == 0) return 0;
    return (int32_t)lust_wsa_last();
}

int32_t lust_win_socket_dup(uintptr_t sock) {
    int e = lust_wsa_startup();
    if (e) return -e;
    WSAPROTOCOL_INFOW info;
    memset(&info, 0, sizeof info);
    if (WSADuplicateSocketW((SOCKET)sock, GetCurrentProcessId(), &info) != 0) return -(int32_t)lust_wsa_last();
    SOCKET n = WSASocketW(info.iAddressFamily, info.iSocketType, info.iProtocol, &info, 0,
                          WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT);
    if (n == INVALID_SOCKET) {
        e = lust_wsa_last();
        if (e != WSAEPROTOTYPE && e != WSAEINVAL) return -(int32_t)e;
        n = WSASocketW(info.iAddressFamily, info.iSocketType, info.iProtocol, &info, 0, WSA_FLAG_OVERLAPPED);
        if (n == INVALID_SOCKET) return -(int32_t)lust_wsa_last();
        if (!SetHandleInformation((HANDLE)(uintptr_t)n, 1, 0)) {
            e = (int)GetLastError();
            closesocket(n);
            return -(int32_t)(e ? e : WSAEINVAL);
        }
    }
    return lust_win_fd_new_socket((uintptr_t)n);
}

int32_t lust_sys_net_resolve(const uint8_t *host, uint64_t hostlen, uint64_t port, lust_Vec *out) {
    int e = lust_wsa_startup();
    if (e) return -e;
    char *name = malloc((size_t)hostlen + 1);
    if (!name) lust_oom();
    memcpy(name, host, (size_t)hostlen);
    name[hostlen] = 0;
    if (memchr(name, 0, (size_t)hostlen)) {
        free(name);
        return -WSAEINVAL;
    }
    struct addrinfo hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_socktype = SOCK_STREAM;
    struct addrinfo *res = 0;
    int rc = getaddrinfo(name, 0, &hints, &res);
    free(name);
    if (rc != 0) {
        e = WSAGetLastError();
        if (e == 0) e = rc;
        return -e;
    }
    int32_t n = 0;
    for (struct addrinfo *ai = res; ai; ai = ai->ai_next) {
        if (ai->ai_family == AF_INET) ((struct sockaddr_in *)ai->ai_addr)->sin_port = htons((uint16_t)port);
        else if (ai->ai_family == AF_INET6) ((struct sockaddr_in6 *)ai->ai_addr)->sin6_port = htons((uint16_t)port);
        else continue;
        lust_sysnet_push_addr(out, ai->ai_addr);
        n++;
    }
    freeaddrinfo(res);
    return n;
}

static int32_t lust_sysnet_bind(const uint8_t *rec, uint64_t len, lust_Vec *out, int ty, int backlog) {
    int fam = lust_sysnet_family(rec, len);
    if (fam < 0) return -WSAEINVAL;
    SOCKET s = INVALID_SOCKET;
    int e = lust_wsa_open(&s, fam, ty);
    if (e) return -e;
    struct sockaddr_storage ss;
    int sl = lust_sysnet_read_addr(rec, len, &ss);
    if (bind(s, (struct sockaddr *)&ss, sl) != 0) {
        e = lust_wsa_last();
        closesocket(s);
        return -e;
    }
    /* No SO_REUSEADDR: on Windows that flag allows socket hijacking,
       and rustc does not set it. IPV6_V6ONLY stays at the Windows
       default (on). */
    if (backlog >= 0 && listen(s, backlog) != 0) {
        e = lust_wsa_last();
        closesocket(s);
        return -e;
    }
    int32_t fd = lust_win_fd_new_socket((uintptr_t)s);
    lust_push_local(s, out);
    return fd;
}

int32_t lust_sys_net_tcp_bind(const uint8_t *rec, uint64_t len, lust_Vec *out) {
    return lust_sysnet_bind(rec, len, out, SOCK_STREAM, 128);
}

int32_t lust_sys_net_udp_bind(const uint8_t *rec, uint64_t len, lust_Vec *out) {
    return lust_sysnet_bind(rec, len, out, SOCK_DGRAM, -1);
}

int32_t lust_sys_net_tcp_connect(const uint8_t *rec, uint64_t len, lust_Vec *out) {
    int fam = lust_sysnet_family(rec, len);
    if (fam < 0) return -WSAEINVAL;
    SOCKET s = INVALID_SOCKET;
    int e = lust_wsa_open(&s, fam, SOCK_STREAM);
    if (e) return -e;
    struct sockaddr_storage ss;
    int sl = lust_sysnet_read_addr(rec, len, &ss);
    if (connect(s, (struct sockaddr *)&ss, sl) != 0) {
        e = lust_wsa_last();
        closesocket(s);
        return -e;
    }
    int32_t fd = lust_win_fd_new_socket((uintptr_t)s);
    lust_push_local(s, out);
    return fd;
}

int32_t lust_sys_net_tcp_connect_timeout(const uint8_t *rec, uint64_t len, int64_t timeout_us, lust_Vec *out) {
    int fam = lust_sysnet_family(rec, len);
    if (fam < 0) return -WSAEINVAL;
    SOCKET s = INVALID_SOCKET;
    int e = lust_wsa_open(&s, fam, SOCK_STREAM);
    if (e) return -e;
    e = lust_set_nonblock(s, 1);
    if (e) { closesocket(s); return -e; }
    struct sockaddr_storage ss;
    int sl = lust_sysnet_read_addr(rec, len, &ss);
    int cr = connect(s, (struct sockaddr *)&ss, sl);
    int ce = cr == 0 ? 0 : lust_wsa_last();
    /* rustc puts the socket back into blocking mode before it looks
       at the connect result. */
    e = lust_set_nonblock(s, 0);
    if (e) { closesocket(s); return -e; }
    if (cr != 0 && ce != WSAEWOULDBLOCK) { closesocket(s); return -ce; }
    if (cr != 0) {
        if (timeout_us <= 0) { closesocket(s); return -10001; }
        fd_set wr, er;
        memset(&wr, 0, sizeof wr);
        wr.fd_count = 1;
        wr.fd_array[0] = s;
        er = wr;
        struct timeval tv;
        int64_t sec = timeout_us / 1000000;
        if (sec > 2147483647) sec = 2147483647;
        tv.tv_sec = (long)sec;
        tv.tv_usec = (long)(timeout_us % 1000000);
        if (tv.tv_sec == 0 && tv.tv_usec == 0) tv.tv_usec = 1;
        int count = select(1, 0, &wr, &er, &tv);
        if (count == SOCKET_ERROR) { e = lust_wsa_last(); closesocket(s); return -e; }
        if (count == 0) { closesocket(s); return -10000; }
        if (wr.fd_count != 1) {
            int err = 0;
            int el = (int)sizeof err;
            if (getsockopt(s, SOL_SOCKET, SO_ERROR, (char *)&err, &el) != 0) {
                e = lust_wsa_last();
                closesocket(s);
                return -e;
            }
            if (err != 0) { closesocket(s); return -err; }
        }
    }
    int32_t fd = lust_win_fd_new_socket((uintptr_t)s);
    lust_push_local(s, out);
    return fd;
}

int32_t lust_sys_net_connect_fd(int32_t fd, const uint8_t *rec, uint64_t len) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return e;
    struct sockaddr_storage ss;
    int sl = lust_sysnet_read_addr(rec, len, &ss);
    if (sl == 0) return WSAEINVAL;
    return connect(s, (struct sockaddr *)&ss, sl) == 0 ? 0 : lust_wsa_last();
}

int32_t lust_sys_net_accept(int32_t fd, lust_Vec *out) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return -e;
    struct sockaddr_storage ss;
    memset(&ss, 0, sizeof ss);
    int ll = (int)sizeof ss;
    SOCKET n = accept(s, (struct sockaddr *)&ss, &ll);
    if (n == INVALID_SOCKET) return -lust_wsa_last();
    int32_t nfd = lust_win_fd_new_socket((uintptr_t)n);
    lust_sysnet_push_addr(out, (struct sockaddr *)&ss);
    return nfd;
}

int32_t lust_sys_net_addr_of(int32_t fd, int32_t peer, lust_Vec *out) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return e;
    struct sockaddr_storage ss;
    memset(&ss, 0, sizeof ss);
    int ll = (int)sizeof ss;
    int rc = peer ? getpeername(s, (struct sockaddr *)&ss, &ll) : getsockname(s, (struct sockaddr *)&ss, &ll);
    if (rc != 0) return lust_wsa_last();
    lust_sysnet_push_addr(out, (struct sockaddr *)&ss);
    return 0;
}

int32_t lust_sys_net_shutdown(int32_t fd, int32_t how) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return e;
    int wh = how == 0 ? SD_RECEIVE : how == 1 ? SD_SEND : SD_BOTH;
    return shutdown(s, wh) == 0 ? 0 : lust_wsa_last();
}

int32_t lust_sys_net_setopt(int32_t fd, int32_t opt, int64_t val) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return e;
    int v = (int)val;
    switch (opt) {
        case 0: return setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&v, (int)sizeof v) == 0 ? 0 : lust_wsa_last();
        case 1: return setsockopt(s, IPPROTO_IP, IP_TTL, (const char *)&v, (int)sizeof v) == 0 ? 0 : lust_wsa_last();
        case 2: return setsockopt(s, SOL_SOCKET, SO_BROADCAST, (const char *)&v, (int)sizeof v) == 0 ? 0 : lust_wsa_last();
        case 3:
            e = lust_set_nonblock(s, val != 0);
            if (e) return e;
            lust_win_fd_set_nonblock(fd, val != 0);
            return 0;
        case 4:
        case 5: {
            DWORD ms = lust_ms_from_us(val);
            return setsockopt(s, SOL_SOCKET, opt == 4 ? SO_RCVTIMEO : SO_SNDTIMEO, (const char *)&ms, (int)sizeof ms) == 0 ? 0 : lust_wsa_last();
        }
        default: return WSAEINVAL;
    }
}

int64_t lust_sys_net_getopt(int32_t fd, int32_t opt) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return -(int64_t)e;
    int v = 0;
    int ll = (int)sizeof v;
    switch (opt) {
        case 0: return getsockopt(s, IPPROTO_TCP, TCP_NODELAY, (char *)&v, &ll) == 0 ? (v != 0) : -(int64_t)lust_wsa_last();
        case 1: return getsockopt(s, IPPROTO_IP, IP_TTL, (char *)&v, &ll) == 0 ? v : -(int64_t)lust_wsa_last();
        case 2: return getsockopt(s, SOL_SOCKET, SO_BROADCAST, (char *)&v, &ll) == 0 ? (v != 0) : -(int64_t)lust_wsa_last();
        case 3: return lust_win_fd_nonblock(fd);
        case 4:
        case 5: {
            DWORD ms = 0;
            int ml = (int)sizeof ms;
            if (getsockopt(s, SOL_SOCKET, opt == 4 ? SO_RCVTIMEO : SO_SNDTIMEO, (char *)&ms, &ml) != 0) return -(int64_t)lust_wsa_last();
            return (int64_t)((uint64_t)ms * 1000u);
        }
        default: return -(int64_t)WSAEINVAL;
    }
}

int64_t lust_sys_net_sendto(int32_t fd, const uint8_t *p, uint64_t len, const uint8_t *rec, uint64_t rlen) {
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return -(int64_t)e;
    int nlen = len > 0x7fffffff ? 0x7fffffff : (int)len;
    int n;
    if (rlen == 0) n = send(s, (const char *)p, nlen, 0);
    else {
        struct sockaddr_storage ss;
        int sl = lust_sysnet_read_addr(rec, rlen, &ss);
        if (sl == 0) return -(int64_t)WSAEINVAL;
        n = sendto(s, (const char *)p, nlen, 0, (struct sockaddr *)&ss, sl);
    }
    return n == SOCKET_ERROR ? -(int64_t)lust_wsa_last() : n;
}

int64_t lust_sys_net_recvfrom(int32_t fd, uint64_t max, int32_t flags, lust_Vec *out, lust_Vec *addrout) {
    if (max == 0) return 0;
    SOCKET s;
    int e = lust_take_socket(fd, &s);
    if (e) return -(int64_t)e;
    uint32_t want = (uint32_t)(max > 0x7fffffff ? 0x7fffffff : max);
    uint8_t *tmp = malloc((size_t)want);
    if (!tmp) lust_oom();
    struct sockaddr_storage ss;
    memset(&ss, 0, sizeof ss);
    int ll = (int)sizeof ss;
    int n = recvfrom(s, (char *)tmp, (int)want, flags & 1 ? MSG_PEEK : 0, (struct sockaddr *)&ss, &ll);
    if (n == SOCKET_ERROR) {
        e = WSAGetLastError();
        free(tmp);
        if (e == WSAESHUTDOWN) {
            lust_sysnet_put64(addrout, 4);
            lust_sysnet_put64(addrout, 0);
            lust_sysnet_put64(addrout, 0);
            lust_sysnet_put64(addrout, 0);
            lust_sysnet_put64(addrout, 0);
            lust_sysnet_put64(addrout, 0);
            return 0;
        }
        return -(int64_t)(e ? e : WSAEINVAL);
    }
    lust_sysnet_push(out, tmp, (uint64_t)n);
    free(tmp);
    lust_sysnet_push_addr(addrout, (struct sockaddr *)&ss);
    return n;
}

int32_t lust_sys_net_unix_pair(lust_Vec *out) { (void)out; return -50; }
int32_t lust_sys_net_unix_bind(const uint8_t *p, uint64_t len, int32_t abstract) { (void)p; (void)len; (void)abstract; return -50; }
int32_t lust_sys_net_unix_connect(const uint8_t *p, uint64_t len, int32_t abstract) { (void)p; (void)len; (void)abstract; return -50; }
int32_t lust_sys_net_unix_accept(int32_t fd, lust_Vec *out) { (void)fd; (void)out; return -50; }
int32_t lust_sys_net_unix_addr(int32_t fd, int32_t peer, lust_Vec *out) { (void)fd; (void)peer; (void)out; return 50; }
