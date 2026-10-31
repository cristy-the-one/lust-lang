/* The socket runtime (w-cinterop): the C behind the `sys_net_*`
   intrinsics of src/ty/lang.rs, called from library/std/src/net.rs.
   A socket address crosses as one 48-byte record of six 8-byte
   little-endian fields: family (4 or 6), port (host order), the
   address bytes in the next two fields (network octet order from the
   low byte), flowinfo, and scope id. Output addresses append one
   record to the Vec. OS errors return as errno (positive), or as
   -errno where the value is a count or an fd.

   Declarations here are in every unit; bodies (sysnet.c) only in the
   runtime unit (LUST_IMPL), like sysio.h. */
#ifndef LUST_RT_SYSNET_H
#define LUST_RT_SYSNET_H

/* Resolves a host name with getaddrinfo (AF_UNSPEC, SOCK_STREAM, as
   std's lookup): appends one address record per result, with `port`
   set on each, and returns the count. On Unix a failure returns -1
   and the Vec holds the gai_strerror text. On Windows a failure
   returns -WSAGetLastError() and the Vec is left empty; std maps
   that with from_raw_os_error, as rustc's cvt_gai does. */
int32_t lust_sys_net_resolve(const uint8_t *host, uint64_t hostlen, uint64_t port, lust_Vec *out);
/* socket + SO_REUSEADDR + bind + listen(128) for one address record:
   the fd or -errno. On success the local address record is appended. */
int32_t lust_sys_net_tcp_bind(const uint8_t *rec, uint64_t len, lust_Vec *out);
/* socket + bind for one address record (UDP): the fd or -errno. */
int32_t lust_sys_net_udp_bind(const uint8_t *rec, uint64_t len, lust_Vec *out);
/* socket + connect to one address record (TCP): the fd or -errno. */
int32_t lust_sys_net_tcp_connect(const uint8_t *rec, uint64_t len, lust_Vec *out);
/* socket + nonblocking connect + poll(timeout_us) for one record:
   the fd in blocking mode, -errno, or -10000 when the poll timed
   out. The local address record is appended on success. */
int32_t lust_sys_net_tcp_connect_timeout(const uint8_t *rec, uint64_t len, int64_t timeout_us, lust_Vec *out);
/* connect(2) of an existing fd to an address record: 0 or errno. */
int32_t lust_sys_net_connect_fd(int32_t fd, const uint8_t *rec, uint64_t len);
/* accept4 with SOCK_CLOEXEC: the new fd or -errno; the peer record
   is appended on success. */
int32_t lust_sys_net_accept(int32_t fd, lust_Vec *out);
/* getsockname (peer=0) or getpeername (peer=1): 0 or errno; the
   address record is appended on success. */
int32_t lust_sys_net_addr_of(int32_t fd, int32_t peer, lust_Vec *out);
/* shutdown(2): how is 0 read, 1 write, 2 both. 0 or errno. */
int32_t lust_sys_net_shutdown(int32_t fd, int32_t how);
/* One socket option by number: 0 nodelay, 1 ttl, 2 broadcast,
   3 nonblocking, 4 read timeout, 5 write timeout (timeouts in
   microseconds, 0 is none). Set returns 0 or errno; get returns
   the value (a bool as 0/1, a count, microseconds) or -errno. */
int32_t lust_sys_net_setopt(int32_t fd, int32_t opt, int64_t val);
int64_t lust_sys_net_getopt(int32_t fd, int32_t opt);
/* sendto(2); an empty address record is a plain send(2): the count
   sent, or -errno. */
int64_t lust_sys_net_sendto(int32_t fd, const uint8_t *p, uint64_t len, const uint8_t *rec, uint64_t rlen);
/* recvfrom(2) of up to `max` bytes appended to `out`, with the
   source address record appended to `addrout`: the count, or
   -errno. `flags` bit 0 is MSG_PEEK. */
int64_t lust_sys_net_recvfrom(int32_t fd, uint64_t max, int32_t flags, lust_Vec *out, lust_Vec *addrout);
/* Unix domain sockets (std::os::unix::net). A path crosses as
   bytes plus an abstract flag (1 = abstract name, no leading NUL
   in the bytes). An address record out is one u64 kind (0 unnamed,
   1 pathname, 2 abstract) then the name bytes (no NULs). */
/* socketpair(AF_UNIX, SOCK_STREAM): the first fd or -errno; the
   second fd appended as one u64 field. */
int32_t lust_sys_net_unix_pair(lust_Vec *out);
/* socket + bind + listen(128) at a unix path: the fd or -errno. */
int32_t lust_sys_net_unix_bind(const uint8_t *p, uint64_t len, int32_t abstract);
/* socket + connect to a unix path: the fd or -errno. */
int32_t lust_sys_net_unix_connect(const uint8_t *p, uint64_t len, int32_t abstract);
/* accept4 on a unix listener: the new fd or -errno; the peer
   address record is appended. */
int32_t lust_sys_net_unix_accept(int32_t fd, lust_Vec *out);
/* getsockname (peer=0) or getpeername (peer=1) of a unix socket:
   0 or errno; the address record is appended. */
int32_t lust_sys_net_unix_addr(int32_t fd, int32_t peer, lust_Vec *out);

#ifdef LUST_IMPL
#include "sysnet.c"
#endif

#endif
