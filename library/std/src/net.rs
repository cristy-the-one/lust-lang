// `std::net` overlay (docs/ARCH.md section 3.1; W-STD-C phase 2):
// the address types, plus the sockets at the end of the file
// (w-cinterop, over the sysnet intrinsics). `std::net::X` walks
// into this module only for the names it defines
// (`STD_MODULES` in src/stdmods.rs).
//
// Lust std source with the stable, observable behavior of
// rust-lang/rust 1.99 `std/src/net` (`addr.rs`, `ip_addr.rs`,
// `parser.rs`): values, `Display` text (with padding, through
// `Formatter::pad` as std's buffer form does), parse accepts and
// rejects, and the `AddrParseError` texts. The unstable `ip`
// predicates (`is_global`, `is_reserved`, and kin) are not here.

use core::str::FromStr;

/// Pushes the decimal text of `v` on `out`.
fn push_dec(out: &mut String, v: u32) {
    if v >= 10 {
        push_dec(out, v / 10);
    }
    out.push((b'0' + (v % 10) as u8) as char);
}

/// Pushes the lowercase hex text of `v`, no leading zeros.
fn push_hex(out: &mut String, v: u16) {
    if v >= 16 {
        push_hex(out, v / 16);
    }
    let d = (v % 16) as u8;
    out.push(if d < 10 { (b'0' + d) as char } else { (b'a' + d - 10) as char });
}

/// An IPv4 address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv4Addr {
    octets: [u8; 4],
}

/// An IPv6 address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ipv6Addr {
    octets: [u8; 16],
}

/// Std's hash: the octets as one native-endian `u32`
/// (`u32::from_ne_bytes`; lust's targets are little-endian), not a byte
/// slice.
impl core::hash::Hash for Ipv4Addr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        let mut v = 0u32;
        let mut i = 4;
        while i > 0 {
            i -= 1;
            v = (v << 8) | self.octets[i] as u32;
        }
        v.hash(state);
    }
}

/// Std's hash: the octets as one native-endian `u128`.
impl core::hash::Hash for Ipv6Addr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        let mut v = 0u128;
        let mut i = 16;
        while i > 0 {
            i -= 1;
            v = (v << 8) | self.octets[i] as u128;
        }
        v.hash(state);
    }
}

/// An IP address, either IPv4 or IPv6.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

/// A socket address: an IPv4 address and a port.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketAddrV4 {
    ip: Ipv4Addr,
    port: u16,
}

/// A socket address: an IPv6 address, a port, flow info, and a
/// scope id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketAddrV6 {
    ip: Ipv6Addr,
    port: u16,
    flowinfo: u32,
    scope_id: u32,
}

/// A socket address, either IPv4 or IPv6.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

// The parse error kinds, in one byte: std's private
// `AddrParseErrorKind`. The numbers only name the texts below.
const ERR_IP: u8 = 0;
const ERR_IPV4: u8 = 1;
const ERR_IPV6: u8 = 2;
const ERR_SOCKET: u8 = 3;
const ERR_SOCKET_V4: u8 = 4;
const ERR_SOCKET_V6: u8 = 5;

/// The error of parsing an IP or socket address.
#[derive(Clone, PartialEq, Eq)]
pub struct AddrParseError {
    kind: u8,
}

impl AddrParseError {
    fn text(&self) -> &'static str {
        match self.kind {
            ERR_IPV4 => "invalid IPv4 address syntax",
            ERR_IPV6 => "invalid IPv6 address syntax",
            ERR_SOCKET_V4 => "invalid IPv4 socket address syntax",
            ERR_SOCKET_V6 => "invalid IPv6 socket address syntax",
            ERR_SOCKET => "invalid socket address syntax",
            _ => "invalid IP address syntax",
        }
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            ERR_IPV4 => "Ipv4",
            ERR_IPV6 => "Ipv6",
            ERR_SOCKET_V4 => "SocketV4",
            ERR_SOCKET_V6 => "SocketV6",
            ERR_SOCKET => "Socket",
            _ => "Ip",
        }
    }
}

impl core::fmt::Display for AddrParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.text())
    }
}

/// The derived Debug of std's private `AddrKind` variant (`Ip`).
struct AddrKind(&'static str);

impl core::fmt::Debug for AddrKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

/// Std derives it over `AddrParseError(AddrKind)`.
impl core::fmt::Debug for AddrParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("AddrParseError").field(&AddrKind(self.kind_name())).finish()
    }
}

impl core::error::Error for AddrParseError {}

impl Ipv4Addr {
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr { octets: [a, b, c, d] }
    }

    pub const LOCALHOST: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
    pub const UNSPECIFIED: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
    pub const BROADCAST: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 255);

    pub const fn octets(&self) -> [u8; 4] {
        self.octets
    }

    pub const fn from_bits(bits: u32) -> Ipv4Addr {
        Ipv4Addr { octets: [(bits >> 24) as u8, (bits >> 16) as u8, (bits >> 8) as u8, bits as u8] }
    }

    pub const fn to_bits(self) -> u32 {
        ((self.octets[0] as u32) << 24) | ((self.octets[1] as u32) << 16) | ((self.octets[2] as u32) << 8) | self.octets[3] as u32
    }

    pub const fn is_unspecified(&self) -> bool {
        self.octets[0] == 0 && self.octets[1] == 0 && self.octets[2] == 0 && self.octets[3] == 0
    }

    pub const fn is_loopback(&self) -> bool {
        self.octets[0] == 127
    }

    pub const fn is_private(&self) -> bool {
        self.octets[0] == 10 || (self.octets[0] == 172 && self.octets[1] >= 16 && self.octets[1] < 32) || (self.octets[0] == 192 && self.octets[1] == 168)
    }

    pub const fn is_link_local(&self) -> bool {
        self.octets[0] == 169 && self.octets[1] == 254
    }

    pub const fn is_multicast(&self) -> bool {
        self.octets[0] >= 224 && self.octets[0] < 240
    }

    pub const fn is_broadcast(&self) -> bool {
        self.octets[0] == 255 && self.octets[1] == 255 && self.octets[2] == 255 && self.octets[3] == 255
    }

    pub const fn is_documentation(&self) -> bool {
        (self.octets[0] == 192 && self.octets[1] == 0 && self.octets[2] == 2)
            || (self.octets[0] == 198 && self.octets[1] == 51 && self.octets[2] == 100)
            || (self.octets[0] == 203 && self.octets[1] == 0 && self.octets[2] == 113)
    }

    pub const fn to_ipv6_compatible(&self) -> Ipv6Addr {
        let o = self.octets;
        Ipv6Addr { octets: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, o[0], o[1], o[2], o[3]] }
    }

    pub const fn to_ipv6_mapped(&self) -> Ipv6Addr {
        let o = self.octets;
        Ipv6Addr { octets: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, o[0], o[1], o[2], o[3]] }
    }

    /// The `Display` text.
    fn text(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.octets[0] as u32);
        s.push('.');
        push_dec(&mut s, self.octets[1] as u32);
        s.push('.');
        push_dec(&mut s, self.octets[2] as u32);
        s.push('.');
        push_dec(&mut s, self.octets[3] as u32);
        s
    }
}

impl Ipv6Addr {
    pub const fn new(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16) -> Ipv6Addr {
        Ipv6Addr {
            octets: [
                (a >> 8) as u8, a as u8, (b >> 8) as u8, b as u8, (c >> 8) as u8, c as u8, (d >> 8) as u8, d as u8,
                (e >> 8) as u8, e as u8, (f >> 8) as u8, f as u8, (g >> 8) as u8, g as u8, (h >> 8) as u8, h as u8,
            ],
        }
    }

    pub const LOCALHOST: Ipv6Addr = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1);
    pub const UNSPECIFIED: Ipv6Addr = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0);

    pub const fn segments(&self) -> [u16; 8] {
        let o = self.octets;
        [
            ((o[0] as u16) << 8) | o[1] as u16,
            ((o[2] as u16) << 8) | o[3] as u16,
            ((o[4] as u16) << 8) | o[5] as u16,
            ((o[6] as u16) << 8) | o[7] as u16,
            ((o[8] as u16) << 8) | o[9] as u16,
            ((o[10] as u16) << 8) | o[11] as u16,
            ((o[12] as u16) << 8) | o[13] as u16,
            ((o[14] as u16) << 8) | o[15] as u16,
        ]
    }

    pub const fn octets(&self) -> [u8; 16] {
        self.octets
    }

    pub const fn from_bits(bits: u128) -> Ipv6Addr {
        Ipv6Addr {
            octets: [
                (bits >> 120) as u8, (bits >> 112) as u8, (bits >> 104) as u8, (bits >> 96) as u8,
                (bits >> 88) as u8, (bits >> 80) as u8, (bits >> 72) as u8, (bits >> 64) as u8,
                (bits >> 56) as u8, (bits >> 48) as u8, (bits >> 40) as u8, (bits >> 32) as u8,
                (bits >> 24) as u8, (bits >> 16) as u8, (bits >> 8) as u8, bits as u8,
            ],
        }
    }

    pub const fn to_bits(self) -> u128 {
        let o = self.octets;
        ((o[0] as u128) << 120) | ((o[1] as u128) << 112) | ((o[2] as u128) << 104) | ((o[3] as u128) << 96)
            | ((o[4] as u128) << 88) | ((o[5] as u128) << 80) | ((o[6] as u128) << 72) | ((o[7] as u128) << 64)
            | ((o[8] as u128) << 56) | ((o[9] as u128) << 48) | ((o[10] as u128) << 40) | ((o[11] as u128) << 32)
            | ((o[12] as u128) << 24) | ((o[13] as u128) << 16) | ((o[14] as u128) << 8) | o[15] as u128
    }

    pub const fn is_unspecified(&self) -> bool {
        let s = self.segments();
        s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 && s[6] == 0 && s[7] == 0
    }

    pub const fn is_loopback(&self) -> bool {
        let s = self.segments();
        s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 && s[6] == 0 && s[7] == 1
    }

    pub const fn is_multicast(&self) -> bool {
        self.segments()[0] & 0xff00 == 0xff00
    }

    pub const fn is_unique_local(&self) -> bool {
        self.segments()[0] & 0xfe00 == 0xfc00
    }

    pub const fn is_unicast_link_local(&self) -> bool {
        self.segments()[0] & 0xffc0 == 0xfe80
    }

    /// The IPv4 address of an IPv4-mapped address (`::ffff:a.b.c.d`).
    pub const fn to_ipv4_mapped(&self) -> Option<Ipv4Addr> {
        let s = self.segments();
        if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xffff {
            Some(Ipv4Addr { octets: [self.octets[12], self.octets[13], self.octets[14], self.octets[15]] })
        } else {
            None
        }
    }

    /// The IPv4 address of an IPv4-compatible or IPv4-mapped
    /// address.
    pub const fn to_ipv4(&self) -> Option<Ipv4Addr> {
        let s = self.segments();
        if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && (s[5] == 0 || s[5] == 0xffff) {
            Some(Ipv4Addr { octets: [self.octets[12], self.octets[13], self.octets[14], self.octets[15]] })
        } else {
            None
        }
    }

    /// The `Display` text: std's form. An IPv4-mapped address
    /// prints its last 32 bits as IPv4; otherwise the longest run
    /// of two or more zero segments (the first, on a tie) becomes
    /// `::`.
    fn text(&self) -> String {
        let seg = self.segments();
        if let Some(v4) = self.to_ipv4_mapped() {
            let mut s = String::new();
            s.push_str("::ffff:");
            s.push_str(&v4.text());
            return s;
        }
        // The best zero run: (start, len), len at least 2.
        let mut best_start = 8usize;
        let mut best_len = 1usize;
        let mut cur_start = 8usize;
        let mut cur_len = 0usize;
        let mut i = 0;
        while i < 8 {
            if seg[i] == 0 {
                if cur_len == 0 {
                    cur_start = i;
                }
                cur_len += 1;
                if cur_len > best_len {
                    best_start = cur_start;
                    best_len = cur_len;
                }
            } else {
                cur_len = 0;
                cur_start = 8;
            }
            i += 1;
        }
        let mut s = String::new();
        let mut i = 0;
        let mut need_colon = false;
        while i < 8 {
            if i == best_start {
                s.push_str("::");
                need_colon = false;
                i += best_len;
                if i >= 8 {
                    break;
                }
                continue;
            }
            if need_colon {
                s.push(':');
            }
            push_hex(&mut s, seg[i]);
            need_colon = true;
            i += 1;
        }
        s
    }
}

impl IpAddr {
    pub const fn is_ipv4(&self) -> bool {
        match self {
            IpAddr::V4(_) => true,
            IpAddr::V6(_) => false,
        }
    }

    pub const fn is_ipv6(&self) -> bool {
        match self {
            IpAddr::V4(_) => false,
            IpAddr::V6(_) => true,
        }
    }

    pub const fn is_unspecified(&self) -> bool {
        match self {
            IpAddr::V4(a) => a.is_unspecified(),
            IpAddr::V6(a) => a.is_unspecified(),
        }
    }

    pub const fn is_loopback(&self) -> bool {
        match self {
            IpAddr::V4(a) => a.is_loopback(),
            IpAddr::V6(a) => a.is_loopback(),
        }
    }

    pub const fn is_multicast(&self) -> bool {
        match self {
            IpAddr::V4(a) => a.is_multicast(),
            IpAddr::V6(a) => a.is_multicast(),
        }
    }

    /// The canonical form: an IPv4-mapped IPv6 address becomes
    /// its IPv4 form, every other address stays as it is.
    pub const fn to_canonical(self) -> IpAddr {
        match self {
            IpAddr::V6(a) => match a.to_ipv4_mapped() {
                Some(v4) => IpAddr::V4(v4),
                None => IpAddr::V6(a),
            },
            IpAddr::V4(_) => self,
        }
    }

    fn text(&self) -> String {
        match self {
            IpAddr::V4(a) => a.text(),
            IpAddr::V6(a) => a.text(),
        }
    }
}

impl SocketAddrV4 {
    pub const fn new(ip: Ipv4Addr, port: u16) -> SocketAddrV4 {
        SocketAddrV4 { ip, port }
    }

    pub const fn ip(&self) -> &Ipv4Addr {
        &self.ip
    }

    pub fn set_ip(&mut self, new_ip: Ipv4Addr) {
        self.ip = new_ip;
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn set_port(&mut self, new_port: u16) {
        self.port = new_port;
    }

    fn text(&self) -> String {
        let mut s = self.ip.text();
        s.push(':');
        push_dec(&mut s, self.port as u32);
        s
    }
}

impl SocketAddrV6 {
    pub const fn new(ip: Ipv6Addr, port: u16, flowinfo: u32, scope_id: u32) -> SocketAddrV6 {
        SocketAddrV6 { ip, port, flowinfo, scope_id }
    }

    pub const fn ip(&self) -> &Ipv6Addr {
        &self.ip
    }

    pub fn set_ip(&mut self, new_ip: Ipv6Addr) {
        self.ip = new_ip;
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    pub fn set_port(&mut self, new_port: u16) {
        self.port = new_port;
    }

    pub const fn flowinfo(&self) -> u32 {
        self.flowinfo
    }

    pub fn set_flowinfo(&mut self, new_flowinfo: u32) {
        self.flowinfo = new_flowinfo;
    }

    pub const fn scope_id(&self) -> u32 {
        self.scope_id
    }

    pub fn set_scope_id(&mut self, new_scope_id: u32) {
        self.scope_id = new_scope_id;
    }

    fn text(&self) -> String {
        let mut s = String::new();
        s.push('[');
        s.push_str(&self.ip.text());
        if self.scope_id != 0 {
            s.push('%');
            push_dec(&mut s, self.scope_id);
        }
        s.push_str("]:");
        push_dec(&mut s, self.port as u32);
        s
    }
}

impl SocketAddr {
    pub const fn ip(&self) -> IpAddr {
        match self {
            SocketAddr::V4(a) => IpAddr::V4(*a.ip()),
            SocketAddr::V6(a) => IpAddr::V6(*a.ip()),
        }
    }

    pub fn set_ip(&mut self, new_ip: IpAddr) {
        match (self, new_ip) {
            (SocketAddr::V4(a), IpAddr::V4(ip)) => a.set_ip(ip),
            (SocketAddr::V6(a), IpAddr::V6(ip)) => a.set_ip(ip),
            (me, IpAddr::V4(ip)) => *me = SocketAddr::V4(SocketAddrV4::new(ip, me.port())),
            (me, IpAddr::V6(ip)) => {
                let port = me.port();
                *me = SocketAddr::V6(SocketAddrV6::new(ip, port, 0, 0));
            }
        }
    }

    pub const fn port(&self) -> u16 {
        match self {
            SocketAddr::V4(a) => a.port(),
            SocketAddr::V6(a) => a.port(),
        }
    }

    pub fn set_port(&mut self, new_port: u16) {
        match self {
            SocketAddr::V4(a) => a.set_port(new_port),
            SocketAddr::V6(a) => a.set_port(new_port),
        }
    }

    pub const fn is_ipv4(&self) -> bool {
        match self {
            SocketAddr::V4(_) => true,
            SocketAddr::V6(_) => false,
        }
    }

    pub const fn is_ipv6(&self) -> bool {
        match self {
            SocketAddr::V4(_) => false,
            SocketAddr::V6(_) => true,
        }
    }

    fn text(&self) -> String {
        match self {
            SocketAddr::V4(a) => a.text(),
            SocketAddr::V6(a) => a.text(),
        }
    }
}

// `Display` and `Debug` print the same text, with the formatter's
// padding, as std's do.

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Display for Ipv6Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for Ipv6Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Display for IpAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for IpAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Display for SocketAddrV4 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for SocketAddrV4 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Display for SocketAddrV6 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for SocketAddrV6 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl core::fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.pad(&self.text())
    }
}

impl From<[u8; 4]> for Ipv4Addr {
    fn from(octets: [u8; 4]) -> Ipv4Addr {
        Ipv4Addr { octets }
    }
}

impl From<[u8; 4]> for IpAddr {
    fn from(octets: [u8; 4]) -> IpAddr {
        IpAddr::V4(Ipv4Addr { octets })
    }
}

impl From<[u16; 8]> for Ipv6Addr {
    fn from(seg: [u16; 8]) -> Ipv6Addr {
        Ipv6Addr::new(seg[0], seg[1], seg[2], seg[3], seg[4], seg[5], seg[6], seg[7])
    }
}

impl From<[u16; 8]> for IpAddr {
    fn from(seg: [u16; 8]) -> IpAddr {
        IpAddr::V6(Ipv6Addr::from(seg))
    }
}

impl From<[u8; 16]> for Ipv6Addr {
    fn from(octets: [u8; 16]) -> Ipv6Addr {
        Ipv6Addr { octets }
    }
}

impl From<[u8; 16]> for IpAddr {
    fn from(octets: [u8; 16]) -> IpAddr {
        IpAddr::V6(Ipv6Addr { octets })
    }
}

impl From<Ipv4Addr> for IpAddr {
    fn from(a: Ipv4Addr) -> IpAddr {
        IpAddr::V4(a)
    }
}

impl From<Ipv6Addr> for IpAddr {
    fn from(a: Ipv6Addr) -> IpAddr {
        IpAddr::V6(a)
    }
}

impl From<u32> for Ipv4Addr {
    fn from(bits: u32) -> Ipv4Addr {
        Ipv4Addr::from_bits(bits)
    }
}

impl From<u128> for Ipv6Addr {
    fn from(bits: u128) -> Ipv6Addr {
        Ipv6Addr::from_bits(bits)
    }
}

impl From<Ipv4Addr> for u32 {
    fn from(a: Ipv4Addr) -> u32 {
        a.to_bits()
    }
}

impl From<Ipv6Addr> for u128 {
    fn from(a: Ipv6Addr) -> u128 {
        a.to_bits()
    }
}

impl From<SocketAddrV4> for SocketAddr {
    fn from(a: SocketAddrV4) -> SocketAddr {
        SocketAddr::V4(a)
    }
}

impl From<SocketAddrV6> for SocketAddr {
    fn from(a: SocketAddrV6) -> SocketAddr {
        SocketAddr::V6(a)
    }
}

impl From<(Ipv4Addr, u16)> for SocketAddrV4 {
    fn from(pair: (Ipv4Addr, u16)) -> SocketAddrV4 {
        SocketAddrV4::new(pair.0, pair.1)
    }
}

impl From<(Ipv6Addr, u16)> for SocketAddrV6 {
    fn from(pair: (Ipv6Addr, u16)) -> SocketAddrV6 {
        SocketAddrV6::new(pair.0, pair.1, 0, 0)
    }
}

impl From<(Ipv4Addr, u16)> for SocketAddr {
    fn from(pair: (Ipv4Addr, u16)) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(pair.0, pair.1))
    }
}

impl From<(Ipv6Addr, u16)> for SocketAddr {
    fn from(pair: (Ipv6Addr, u16)) -> SocketAddr {
        SocketAddr::V6(SocketAddrV6::new(pair.0, pair.1, 0, 0))
    }
}

impl From<(IpAddr, u16)> for SocketAddr {
    fn from(pair: (IpAddr, u16)) -> SocketAddr {
        match pair.0 {
            IpAddr::V4(a) => SocketAddr::V4(SocketAddrV4::new(a, pair.1)),
            IpAddr::V6(a) => SocketAddr::V6(SocketAddrV6::new(a, pair.1, 0, 0)),
        }
    }
}

// Parsing, with std's accepts and rejects (std/src/net/parser.rs).
// A byte walk; every error is the one kind of its caller. The
// bytes of the input come from the `sys_str_as_bytes` intrinsic,
// as the platform crate reads them (`str::as_bytes` is not a
// method of the subset yet).

/// The bytes of `s`.
fn bytes_of(s: &str) -> &[u8] {
    core::intrinsics::sys_str_as_bytes(s)
}

/// One IPv4 number at `b[*i..]`: digits, no empty run, no leading
/// zero on more than one digit, at most 255.
fn read_v4_part(b: &[u8], i: &mut usize) -> Option<u8> {
    let start = *i;
    let mut value: u32 = 0;
    let mut digits = 0usize;
    while *i < b.len() && b[*i] >= b'0' && b[*i] <= b'9' {
        value = value * 10 + (b[*i] - b'0') as u32;
        if value > 255 {
            return None;
        }
        digits += 1;
        *i += 1;
    }
    if digits == 0 || (digits > 1 && b[start] == b'0') {
        return None;
    }
    Some(value as u8)
}

/// An IPv4 address from all of `b`.
fn parse_v4(b: &[u8]) -> Option<Ipv4Addr> {
    let mut i = 0usize;
    let a = read_v4_part(b, &mut i)?;
    if i >= b.len() || b[i] != b'.' {
        return None;
    }
    i += 1;
    let c = read_v4_part(b, &mut i)?;
    if i >= b.len() || b[i] != b'.' {
        return None;
    }
    i += 1;
    let d = read_v4_part(b, &mut i)?;
    if i >= b.len() || b[i] != b'.' {
        return None;
    }
    i += 1;
    let e = read_v4_part(b, &mut i)?;
    if i != b.len() {
        return None;
    }
    Some(Ipv4Addr::new(a, c, d, e))
}

/// The value of one hex digit.
fn hex_val(c: u8) -> Option<u16> {
    if c >= b'0' && c <= b'9' {
        Some((c - b'0') as u16)
    } else if c >= b'a' && c <= b'f' {
        Some((c - b'a') as u16 + 10)
    } else if c >= b'A' && c <= b'F' {
        Some((c - b'A') as u16 + 10)
    } else {
        None
    }
}

/// The groups of one side of an IPv6 text: `b` is the whole side
/// with no `::`. An IPv4 tail is allowed only as the last group and
/// only when `tail_ok`; it counts as two groups. Writes the groups
/// into `out` from index 0 and returns their count.
fn read_v6_side(b: &[u8], tail_ok: bool, out: &mut [u16; 8]) -> Option<usize> {
    if b.is_empty() {
        return Some(0);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    loop {
        // One group: read to the next ':' or the end.
        let mut j = i;
        let mut has_dot = false;
        while j < b.len() && b[j] != b':' {
            if b[j] == b'.' {
                has_dot = true;
            }
            j += 1;
        }
        if has_dot {
            if !tail_ok || j != b.len() || n + 1 >= 8 {
                return None;
            }
            let v4 = parse_v4(&b[i..j])?;
            let o = v4.octets;
            out[n] = ((o[0] as u16) << 8) | o[1] as u16;
            out[n + 1] = ((o[2] as u16) << 8) | o[3] as u16;
            return Some(n + 2);
        }
        if j == i || j - i > 4 || n >= 8 {
            return None;
        }
        let mut value: u16 = 0;
        let mut k = i;
        while k < j {
            value = (value << 4) | hex_val(b[k])?;
            k += 1;
        }
        out[n] = value;
        n += 1;
        if j == b.len() {
            return Some(n);
        }
        i = j + 1;
        if i >= b.len() {
            return None;
        }
    }
}

/// An IPv6 address from all of `b`.
fn parse_v6(b: &[u8]) -> Option<Ipv6Addr> {
    // The `::` position, if any; a second one is an error.
    let mut ellipsis: Option<usize> = None;
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b':' && b[i + 1] == b':' {
            if ellipsis.is_some() {
                return None;
            }
            ellipsis = Some(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    let mut seg = [0u16; 8];
    match ellipsis {
        None => {
            if read_v6_side(b, true, &mut seg)? != 8 {
                return None;
            }
        }
        Some(at) => {
            let left = read_v6_side(&b[..at], false, &mut seg)?;
            let mut rseg = [0u16; 8];
            let right = read_v6_side(&b[at + 2..], true, &mut rseg)?;
            if left + right > 7 {
                return None;
            }
            let mut k = 0usize;
            while k < right {
                seg[8 - right + k] = rseg[k];
                k += 1;
            }
        }
    }
    Some(Ipv6Addr::new(seg[0], seg[1], seg[2], seg[3], seg[4], seg[5], seg[6], seg[7]))
}

/// A port from all of `b`: digits, at most 65535.
fn parse_port(b: &[u8]) -> Option<u16> {
    if b.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        value = value * 10 + (b[i] - b'0') as u32;
        if value > 65535 {
            return None;
        }
        i += 1;
    }
    Some(value as u16)
}

/// A scope id from all of `b`: digits that fit a `u32`.
fn parse_scope(b: &[u8]) -> Option<u32> {
    if b.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        value = value * 10 + (b[i] - b'0') as u64;
        if value > 4294967295 {
            return None;
        }
        i += 1;
    }
    Some(value as u32)
}

/// A v4 socket address from all of `b`.
fn parse_sa4(b: &[u8]) -> Option<SocketAddrV4> {
    let mut colon: Option<usize> = None;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b':' {
            if colon.is_some() {
                return None;
            }
            colon = Some(i);
        }
        i += 1;
    }
    let at = colon?;
    let ip = parse_v4(&b[..at])?;
    let port = parse_port(&b[at + 1..])?;
    Some(SocketAddrV4::new(ip, port))
}

/// A v6 socket address from all of `b`: `[` ip (`%` scope)? `]`
/// `:` port.
fn parse_sa6(b: &[u8]) -> Option<SocketAddrV6> {
    if b.is_empty() || b[0] != b'[' {
        return None;
    }
    let mut close: Option<usize> = None;
    let mut i = 1usize;
    while i < b.len() {
        if b[i] == b']' {
            close = Some(i);
            break;
        }
        i += 1;
    }
    let close = close?;
    if close + 1 >= b.len() || b[close + 1] != b':' {
        return None;
    }
    let inner = &b[1..close];
    let mut pct: Option<usize> = None;
    let mut j = 0usize;
    while j < inner.len() {
        if inner[j] == b'%' {
            if pct.is_some() {
                return None;
            }
            pct = Some(j);
        }
        j += 1;
    }
    let (ip, scope) = match pct {
        Some(p) => (parse_v6(&inner[..p])?, parse_scope(&inner[p + 1..])?),
        None => (parse_v6(inner)?, 0),
    };
    let port = parse_port(&b[close + 2..])?;
    Some(SocketAddrV6::new(ip, port, 0, scope))
}

impl FromStr for Ipv4Addr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<Ipv4Addr, AddrParseError> {
        match parse_v4(bytes_of(s)) {
            Some(a) => Ok(a),
            None => Err(AddrParseError { kind: ERR_IPV4 }),
        }
    }
}

impl FromStr for Ipv6Addr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<Ipv6Addr, AddrParseError> {
        match parse_v6(bytes_of(s)) {
            Some(a) => Ok(a),
            None => Err(AddrParseError { kind: ERR_IPV6 }),
        }
    }
}

impl FromStr for IpAddr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<IpAddr, AddrParseError> {
        if let Some(a) = parse_v4(bytes_of(s)) {
            return Ok(IpAddr::V4(a));
        }
        match parse_v6(bytes_of(s)) {
            Some(a) => Ok(IpAddr::V6(a)),
            None => Err(AddrParseError { kind: ERR_IP }),
        }
    }
}

impl FromStr for SocketAddrV4 {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<SocketAddrV4, AddrParseError> {
        match parse_sa4(bytes_of(s)) {
            Some(a) => Ok(a),
            None => Err(AddrParseError { kind: ERR_SOCKET_V4 }),
        }
    }
}

impl FromStr for SocketAddrV6 {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<SocketAddrV6, AddrParseError> {
        match parse_sa6(bytes_of(s)) {
            Some(a) => Ok(a),
            None => Err(AddrParseError { kind: ERR_SOCKET_V6 }),
        }
    }
}

impl FromStr for SocketAddr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<SocketAddr, AddrParseError> {
        if let Some(a) = parse_sa4(bytes_of(s)) {
            return Ok(SocketAddr::V4(a));
        }
        match parse_sa6(bytes_of(s)) {
            Some(a) => Ok(SocketAddr::V6(a)),
            None => Err(AddrParseError { kind: ERR_SOCKET }),
        }
    }
}

// Sockets (w-cinterop): TcpListener, TcpStream, and UdpSocket over
// the `sys_net_*` intrinsics (src/runtime/sysnet.c), with std's
// syscall choices on Linux (SO_REUSEADDR listeners, dual-stack v6
// binds, getaddrinfo order and error text) so results and io::Error
// values match rustc 1.99 std.

use std::io;

/// One address record field, read back from a syscall buffer.
fn rec64(raw: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        v |= (raw[off + i] as u64) << (8 * i);
        i += 1;
    }
    v
}

fn put64(out: &mut Vec<u8>, v: u64) {
    let mut i: usize = 0;
    while i < 8 {
        out.push((v >> (8 * i)) as u8);
        i += 1;
    }
}

/// The eight little-endian bytes of a record field.
fn bytes64(v: u64) -> [u8; 8] {
    let mut b = [0u8; 8];
    let mut i: usize = 0;
    while i < 8 {
        b[i] = (v >> (8 * i)) as u8;
        i += 1;
    }
    b
}

/// A record field from its eight little-endian bytes.
fn word64(b: [u8; 8]) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        v |= (b[i] as u64) << (8 * i);
        i += 1;
    }
    v
}

/// Packs an address as one 48-byte syscall record (sysnet.h).
fn pack_addr(a: &SocketAddr) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    match a {
        SocketAddr::V4(v4) => {
            let o = v4.ip().octets();
            put64(&mut out, 4);
            put64(&mut out, v4.port() as u64);
            put64(&mut out, word64([o[0], o[1], o[2], o[3], 0, 0, 0, 0]));
            put64(&mut out, 0);
            put64(&mut out, 0);
            put64(&mut out, 0);
        }
        SocketAddr::V6(v6) => {
            let o = v6.ip().octets();
            let mut lo = [0u8; 8];
            let mut hi = [0u8; 8];
            let mut i: usize = 0;
            while i < 8 {
                lo[i] = o[i];
                hi[i] = o[8 + i];
                i += 1;
            }
            put64(&mut out, 6);
            put64(&mut out, v6.port() as u64);
            put64(&mut out, word64(lo));
            put64(&mut out, word64(hi));
            put64(&mut out, v6.flowinfo() as u64);
            put64(&mut out, v6.scope_id() as u64);
        }
    }
    out
}

/// One address record back from a syscall buffer.
fn unpack_addr(raw: &[u8], off: usize) -> SocketAddr {
    let fam = rec64(raw, off);
    let port = rec64(raw, off + 8) as u16;
    let lo = bytes64(rec64(raw, off + 16));
    let hi = bytes64(rec64(raw, off + 24));
    if fam == 4 {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(lo[0], lo[1], lo[2], lo[3]), port))
    } else {
        let mut o = [0u8; 16];
        let mut i: usize = 0;
        while i < 8 {
            o[i] = lo[i];
            o[8 + i] = hi[i];
            i += 1;
        }
        SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(o), port, rec64(raw, off + 32) as u32, rec64(raw, off + 40) as u32))
    }
}

fn os_err(code: i32) -> io::Error {
    io::Error::from_raw_os_error(code)
}

/// The result of a status that is 0 or an errno.
fn net_status(code: i32) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(os_err(code)) }
}

/// std's error for a zero duration timeout.
fn zero_timeout_err() -> io::Error {
    io::Error::__lust_adapter_simple_message(io::ErrorKind::InvalidInput, "cannot set a 0 duration timeout")
}

/// A timeout duration in whole microseconds for the sys_net
/// options: sub-microsecond values round up to one, as std's
/// timeval conversion keeps a non-zero timeout non-zero.
fn timeout_us(d: core::time::Duration) -> i64 {
    let us = d.as_secs() as i64 * 1_000_000 + (d.subsec_nanos() / 1000) as i64;
    if us == 0 { 1 } else { us }
}

/// Sets one socket timeout option (4 read, 5 write).
fn set_timeout(fd: i32, opt: i32, timeout: Option<core::time::Duration>) -> io::Result<()> {
    let us = match timeout {
        None => 0,
        Some(d) => {
            if d == core::time::Duration::ZERO {
                return Err(zero_timeout_err());
            }
            timeout_us(d)
        }
    };
    net_status(core::intrinsics::sys_net_setopt(fd, opt, us))
}

/// Reads one socket timeout option (4 read, 5 write).
fn get_timeout(fd: i32, opt: i32) -> io::Result<Option<core::time::Duration>> {
    let v = core::intrinsics::sys_net_getopt(fd, opt);
    if v < 0 {
        Err(os_err((-v) as i32))
    } else if v == 0 {
        Ok(None)
    } else {
        Ok(Some(core::time::Duration::from_micros(v as u64)))
    }
}

/// The local or peer address of a socket.
fn addr_of(fd: i32, peer: bool) -> io::Result<SocketAddr> {
    let mut raw: Vec<u8> = Vec::new();
    match net_status(core::intrinsics::sys_net_addr_of(fd, if peer { 1 } else { 0 }, &mut raw)) {
        Ok(()) => Ok(unpack_addr(&raw, 0)),
        Err(e) => Err(e),
    }
}

/// A getaddrinfo failure, with std's text and kind.
fn lookup_error(text: &str) -> io::Error {
    let mut msg = String::from("failed to lookup address information: ");
    msg.push_str(text);
    io::Error::new(io::ErrorKind::__Uncategorized, msg)
}

/// Resolves a host name and port to addresses, in resolver order.
fn resolve_host(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let mut raw: Vec<u8> = Vec::new();
    let n = core::intrinsics::sys_net_resolve(host.as_bytes(), port as u64, &mut raw);
    if n < 0 {
        // rustc's Windows lookup reports WSAGetLastError(), not gai_strerror.
        if cfg!(windows) {
            return Err(os_err(-n));
        }
        return Err(lookup_error(core::str::from_utf8(&raw).unwrap_or("")));
    }
    let mut out: Vec<SocketAddr> = Vec::new();
    let mut i: i32 = 0;
    while i < n {
        out.push(unpack_addr(&raw, (i as usize) * 48));
        i += 1;
    }
    Ok(out)
}

/// The `str` form of ToSocketAddrs: an address literal first, else
/// a host and port split at the last colon, as std parses it.
fn str_to_addrs(s: &str) -> io::Result<Vec<SocketAddr>> {
    if let Some(a) = parse_sa4(bytes_of(s)) {
        return Ok(vec![SocketAddr::V4(a)]);
    }
    if let Some(a) = parse_sa6(bytes_of(s)) {
        return Ok(vec![SocketAddr::V6(a)]);
    }
    let b = s.as_bytes();
    let mut colon: Option<usize> = None;
    let mut i: usize = 0;
    while i < b.len() {
        if b[i] == b':' {
            colon = Some(i);
        }
        i += 1;
    }
    let idx = match colon {
        Some(idx) => idx,
        None => return Err(io::Error::__lust_adapter_simple_message(io::ErrorKind::InvalidInput, "invalid socket address")),
    };
    let host = &s[..idx];
    let port_str = &s[idx + 1..];
    // A port is decimal digits that fit a u16, as u16's FromStr.
    let port: u16 = {
        let pb = port_str.as_bytes();
        let mut v: u32 = 0;
        let mut bad = pb.is_empty();
        // u16's FromStr takes one leading `+`.
        let mut j: usize = if !pb.is_empty() && pb[0] == b'+' { 1 } else { 0 };
        if j >= pb.len() {
            bad = true;
        }
        while j < pb.len() {
            let c = pb[j];
            if !(b'0' <= c && c <= b'9') {
                bad = true;
            } else {
                v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                if v > 65535 {
                    bad = true;
                }
            }
            j += 1;
        }
        if bad {
            return Err(io::Error::__lust_adapter_simple_message(io::ErrorKind::InvalidInput, "invalid port value"));
        }
        v as u16
    };
    let host = if host.len() >= 2 && host.as_bytes()[0] == b'[' && host.as_bytes()[host.len() - 1] == b']' {
        &host[1..host.len() - 1]
    } else {
        host
    };
    resolve_host(host, port)
}

/// The iterator of every ToSocketAddrs impl here: the resolved list.
pub struct __AddrIter {
    v: Vec<SocketAddr>,
    i: usize,
}

impl Iterator for __AddrIter {
    type Item = SocketAddr;
    fn next(&mut self) -> Option<SocketAddr> {
        if self.i >= self.v.len() {
            return None;
        }
        let a = self.v[self.i];
        self.i += 1;
        Some(a)
    }
}

fn addr_iter(v: Vec<SocketAddr>) -> __AddrIter {
    __AddrIter { v, i: 0 }
}

/// Converts a value to one or more socket addresses, in order.
///
/// std bounds `Iter` by `Iterator<Item = SocketAddr>`; lust cannot
/// state a bound on an associated type yet, so the impls below keep
/// std's contract without the written bound, and `each_addr` states
/// it as a where clause instead.
pub trait ToSocketAddrs {
    type Iter;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter>;
}

impl ToSocketAddrs for SocketAddr {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![*self]))
    }
}

impl ToSocketAddrs for SocketAddrV4 {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![SocketAddr::V4(*self)]))
    }
}

impl ToSocketAddrs for SocketAddrV6 {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![SocketAddr::V6(*self)]))
    }
}

impl ToSocketAddrs for (IpAddr, u16) {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![SocketAddr::from(*self)]))
    }
}

impl ToSocketAddrs for (Ipv4Addr, u16) {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![SocketAddr::from(*self)]))
    }
}

impl ToSocketAddrs for (Ipv6Addr, u16) {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![SocketAddr::from(*self)]))
    }
}

impl ToSocketAddrs for (&str, u16) {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match resolve_host(self.0, self.1) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for (String, u16) {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match resolve_host(&self.0, self.1) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for str {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match str_to_addrs(self) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for String {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match str_to_addrs(self) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for &str {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match str_to_addrs(self) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for &String {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        match str_to_addrs(self) {
            Ok(v) => Ok(addr_iter(v)),
            Err(e) => Err(e),
        }
    }
}

impl ToSocketAddrs for &SocketAddr {
    type Iter = __AddrIter;
    fn to_socket_addrs(&self) -> io::Result<__AddrIter> {
        Ok(addr_iter(vec![**self]))
    }
}

/// Resolves `addr` and runs `f` on each address until one works, as
/// std's bind and connect do; the last error wins.
fn each_addr<A: ToSocketAddrs, T, F: Fn(&SocketAddr) -> io::Result<T>>(addr: A, f: F) -> io::Result<T>
where
    A::Iter: Iterator<Item = SocketAddr>,
{
    let mut last: Option<io::Error> = None;
    let mut iter = addr.to_socket_addrs()?;
    while let Some(a) = iter.next() {
        match f(&a) {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
    }
    match last {
        Some(e) => Err(e),
        None => Err(io::Error::__lust_adapter_simple_message(io::ErrorKind::InvalidInput, "could not resolve to any addresses")),
    }
}

/// Which half of a connection to shut down.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shutdown {
    Read,
    Write,
    Both,
}

impl core::fmt::Debug for Shutdown {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Shutdown::Read => f.write_str("Read"),
            Shutdown::Write => f.write_str("Write"),
            Shutdown::Both => f.write_str("Both"),
        }
    }
}

/// A TCP stream between a local and a remote socket. Dropping it
/// closes the descriptor.
pub struct TcpStream {
    fd: i32,
}

impl TcpStream {
    /// Opens a TCP connection to a remote host.
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream>
    where
        A::Iter: Iterator<Item = SocketAddr>,
    {
        each_addr(addr, |a| {
            let mut raw: Vec<u8> = Vec::new();
            let fd = core::intrinsics::sys_net_tcp_connect(&pack_addr(a), &mut raw);
            if fd < 0 { Err(os_err(-fd)) } else { Ok(TcpStream { fd }) }
        })
    }

    /// Connects to one socket address with a timeout.
    pub fn connect_timeout(addr: &SocketAddr, timeout: core::time::Duration) -> io::Result<TcpStream> {
        // Unix rejects a zero timeout before connect. Windows rustc
        // connects first: an immediate success is Ok, and WouldBlock
        // with a zero timeout is the invalid-input error.
        let us = if timeout == core::time::Duration::ZERO {
            if !cfg!(windows) {
                return Err(zero_timeout_err());
            }
            0
        } else {
            timeout_us(timeout)
        };
        let mut raw: Vec<u8> = Vec::new();
        let fd = core::intrinsics::sys_net_tcp_connect_timeout(&pack_addr(addr), us, &mut raw);
        if fd == -10000 {
            Err(io::Error::new(io::ErrorKind::TimedOut, "connection timed out"))
        } else if fd == -10001 {
            Err(zero_timeout_err())
        } else if fd < 0 {
            Err(os_err(-fd))
        } else {
            Ok(TcpStream { fd })
        }
    }

    /// Sets the read timeout (SO_RCVTIMEO).
    pub fn set_read_timeout(&self, timeout: Option<core::time::Duration>) -> io::Result<()> {
        set_timeout(self.fd, 4, timeout)
    }

    /// The read timeout.
    pub fn read_timeout(&self) -> io::Result<Option<core::time::Duration>> {
        get_timeout(self.fd, 4)
    }

    /// Sets the write timeout (SO_SNDTIMEO).
    pub fn set_write_timeout(&self, timeout: Option<core::time::Duration>) -> io::Result<()> {
        set_timeout(self.fd, 5, timeout)
    }

    /// The write timeout.
    pub fn write_timeout(&self) -> io::Result<Option<core::time::Duration>> {
        get_timeout(self.fd, 5)
    }

    /// Returns the socket address of the remote peer.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        addr_of(self.fd, true)
    }

    /// Returns the socket address of the local half.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        addr_of(self.fd, false)
    }

    /// Shuts down part of the connection.
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        let h = match how {
            Shutdown::Read => 0,
            Shutdown::Write => 1,
            Shutdown::Both => 2,
        };
        net_status(core::intrinsics::sys_net_shutdown(self.fd, h))
    }

    /// A new handle to the same connection.
    pub fn try_clone(&self) -> io::Result<TcpStream> {
        let fd = core::intrinsics::sys_fd_dup(self.fd);
        if fd < 0 { Err(os_err(-fd)) } else { Ok(TcpStream { fd }) }
    }

    /// Moves the socket into or out of nonblocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 3, if nonblocking { 1 } else { 0 }))
    }

    /// Sets TCP_NODELAY.
    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 0, if nodelay { 1 } else { 0 }))
    }

    /// The TCP_NODELAY state.
    pub fn nodelay(&self) -> io::Result<bool> {
        let v = core::intrinsics::sys_net_getopt(self.fd, 0);
        if v < 0 { Err(os_err((-v) as i32)) } else { Ok(v != 0) }
    }

    /// Sets the time-to-live.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 1, ttl as i64))
    }

    /// The time-to-live.
    pub fn ttl(&self) -> io::Result<u32> {
        let v = core::intrinsics::sys_net_getopt(self.fd, 1);
        if v < 0 { Err(os_err((-v) as i32)) } else { Ok(v as u32) }
    }

    /// Receives data without removing it from the queue.
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut tmp: Vec<u8> = Vec::new();
        let mut sink: Vec<u8> = Vec::new();
        let n = core::intrinsics::sys_net_recvfrom(self.fd, buf.len() as u64, 1, &mut tmp, &mut sink);
        if n < 0 {
            return Err(os_err((-n) as i32));
        }
        let n = n as usize;
        let mut i: usize = 0;
        while i < n {
            buf[i] = tmp[i];
            i += 1;
        }
        Ok(n)
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = core::intrinsics::sys_fd_close(self.fd);
        }
    }
}

impl core::fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("TcpStream");
        if let Ok(addr) = self.local_addr() {
            d.field("addr", &addr);
        }
        if let Ok(peer) = self.peer_addr() {
            d.field("peer", &peer);
        }
        d.field("fd", &self.fd).finish()
    }
}

fn read_fd_into(fd: i32, buf: &mut [u8]) -> io::Result<usize> {
    let mut tmp: Vec<u8> = Vec::new();
    let n = core::intrinsics::sys_fd_read(fd, buf.len() as u64, &mut tmp);
    if n < 0 {
        return Err(os_err((-n) as i32));
    }
    let n = n as usize;
    let mut i: usize = 0;
    while i < n {
        buf[i] = tmp[i];
        i += 1;
    }
    Ok(n)
}

fn write_fd(fd: i32, buf: &[u8]) -> io::Result<usize> {
    let n = core::intrinsics::sys_fd_write(fd, buf);
    if n < 0 { Err(os_err((-n) as i32)) } else { Ok(n as usize) }
}

impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_fd_into(self.fd, buf)
    }
}

impl io::Read for &TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_fd_into(self.fd, buf)
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_fd(self.fd, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Write for &TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_fd(self.fd, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A TCP socket server, listening for connections. Dropping it
/// closes the descriptor.
pub struct TcpListener {
    fd: i32,
}

impl TcpListener {
    /// Creates a listening TCP socket bound to the address.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener>
    where
        A::Iter: Iterator<Item = SocketAddr>,
    {
        each_addr(addr, |a| {
            let mut raw: Vec<u8> = Vec::new();
            let fd = core::intrinsics::sys_net_tcp_bind(&pack_addr(a), &mut raw);
            if fd < 0 { Err(os_err(-fd)) } else { Ok(TcpListener { fd }) }
        })
    }

    /// Returns the local socket address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        addr_of(self.fd, false)
    }

    /// Accepts a new incoming connection.
    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let mut raw: Vec<u8> = Vec::new();
        let fd = core::intrinsics::sys_net_accept(self.fd, &mut raw);
        if fd < 0 { Err(os_err(-fd)) } else { Ok((TcpStream { fd }, unpack_addr(&raw, 0))) }
    }

    /// A new handle to the same listener.
    pub fn try_clone(&self) -> io::Result<TcpListener> {
        let fd = core::intrinsics::sys_fd_dup(self.fd);
        if fd < 0 { Err(os_err(-fd)) } else { Ok(TcpListener { fd }) }
    }

    /// Moves the socket into or out of nonblocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 3, if nonblocking { 1 } else { 0 }))
    }

    /// Sets the time-to-live.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 1, ttl as i64))
    }

    /// The time-to-live.
    pub fn ttl(&self) -> io::Result<u32> {
        let v = core::intrinsics::sys_net_getopt(self.fd, 1);
        if v < 0 { Err(os_err((-v) as i32)) } else { Ok(v as u32) }
    }

    /// Returns an iterator over incoming connections.
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming { listener: self }
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = core::intrinsics::sys_fd_close(self.fd);
        }
    }
}

impl core::fmt::Debug for TcpListener {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("TcpListener");
        if let Ok(addr) = self.local_addr() {
            d.field("addr", &addr);
        }
        d.field("fd", &self.fd).finish()
    }
}

/// An iterator over incoming connections to a listener.
pub struct Incoming<'a> {
    listener: &'a TcpListener,
}

impl Iterator for Incoming<'_> {
    type Item = io::Result<TcpStream>;
    fn next(&mut self) -> Option<io::Result<TcpStream>> {
        Some(match self.listener.accept() {
            Ok((s, _)) => Ok(s),
            Err(e) => Err(e),
        })
    }
}

/// Std derives it: `Incoming { listener: TcpListener { .. } }`.
impl core::fmt::Debug for Incoming<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Incoming").field("listener", self.listener).finish()
    }
}

/// A UDP socket. Dropping it closes the descriptor.
pub struct UdpSocket {
    fd: i32,
}

impl UdpSocket {
    /// Creates a UDP socket bound to the address.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket>
    where
        A::Iter: Iterator<Item = SocketAddr>,
    {
        each_addr(addr, |a| {
            let mut raw: Vec<u8> = Vec::new();
            let fd = core::intrinsics::sys_net_udp_bind(&pack_addr(a), &mut raw);
            if fd < 0 { Err(os_err(-fd)) } else { Ok(UdpSocket { fd }) }
        })
    }

    /// Returns the local socket address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        addr_of(self.fd, false)
    }

    /// Returns the remote address this socket is connected to.
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        addr_of(self.fd, true)
    }

    /// Connects the socket to a remote address.
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()>
    where
        A::Iter: Iterator<Item = SocketAddr>,
    {
        each_addr(addr, |a| net_status(core::intrinsics::sys_net_connect_fd(self.fd, &pack_addr(a))))
    }

    /// Sends data to the given address.
    pub fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> io::Result<usize>
    where
        A::Iter: Iterator<Item = SocketAddr>,
    {
        each_addr(addr, |a| {
            let n = core::intrinsics::sys_net_sendto(self.fd, buf, &pack_addr(a));
            if n < 0 { Err(os_err((-n) as i32)) } else { Ok(n as usize) }
        })
    }

    /// Sends data to the connected peer.
    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        let n = core::intrinsics::sys_net_sendto(self.fd, buf, &[]);
        if n < 0 { Err(os_err((-n) as i32)) } else { Ok(n as usize) }
    }

    /// Receives a single datagram and its source address.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let (n, from) = self.recv_inner(buf, 0)?;
        Ok((n, from))
    }

    /// Receives a single datagram from the connected peer.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let (n, _) = self.recv_inner(buf, 0)?;
        Ok(n)
    }

    /// Reads a datagram without removing it from the queue.
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let (n, _) = self.recv_inner(buf, 1)?;
        Ok(n)
    }

    /// Reads a datagram without removing it, with its source.
    pub fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.recv_inner(buf, 1)
    }

    fn recv_inner(&self, buf: &mut [u8], flags: i32) -> io::Result<(usize, SocketAddr)> {
        let mut tmp: Vec<u8> = Vec::new();
        let mut araw: Vec<u8> = Vec::new();
        let n = core::intrinsics::sys_net_recvfrom(self.fd, buf.len() as u64, flags, &mut tmp, &mut araw);
        if n < 0 {
            return Err(os_err((-n) as i32));
        }
        let n = n as usize;
        let mut i: usize = 0;
        while i < n {
            buf[i] = tmp[i];
            i += 1;
        }
        Ok((n, unpack_addr(&araw, 0)))
    }

    /// A new handle to the same socket.
    pub fn try_clone(&self) -> io::Result<UdpSocket> {
        let fd = core::intrinsics::sys_fd_dup(self.fd);
        if fd < 0 { Err(os_err(-fd)) } else { Ok(UdpSocket { fd }) }
    }

    /// Moves the socket into or out of nonblocking mode.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 3, if nonblocking { 1 } else { 0 }))
    }

    /// Sets SO_BROADCAST.
    pub fn set_broadcast(&self, broadcast: bool) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 2, if broadcast { 1 } else { 0 }))
    }

    /// The SO_BROADCAST state.
    pub fn broadcast(&self) -> io::Result<bool> {
        let v = core::intrinsics::sys_net_getopt(self.fd, 2);
        if v < 0 { Err(os_err((-v) as i32)) } else { Ok(v != 0) }
    }

    /// Sets the read timeout (SO_RCVTIMEO).
    pub fn set_read_timeout(&self, timeout: Option<core::time::Duration>) -> io::Result<()> {
        set_timeout(self.fd, 4, timeout)
    }

    /// The read timeout.
    pub fn read_timeout(&self) -> io::Result<Option<core::time::Duration>> {
        get_timeout(self.fd, 4)
    }

    /// Sets the write timeout (SO_SNDTIMEO).
    pub fn set_write_timeout(&self, timeout: Option<core::time::Duration>) -> io::Result<()> {
        set_timeout(self.fd, 5, timeout)
    }

    /// The write timeout.
    pub fn write_timeout(&self) -> io::Result<Option<core::time::Duration>> {
        get_timeout(self.fd, 5)
    }

    /// Sets the time-to-live.
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        net_status(core::intrinsics::sys_net_setopt(self.fd, 1, ttl as i64))
    }

    /// The time-to-live.
    pub fn ttl(&self) -> io::Result<u32> {
        let v = core::intrinsics::sys_net_getopt(self.fd, 1);
        if v < 0 { Err(os_err((-v) as i32)) } else { Ok(v as u32) }
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        if self.fd >= 0 {
            let _ = core::intrinsics::sys_fd_close(self.fd);
        }
    }
}

impl core::fmt::Debug for UdpSocket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("UdpSocket");
        if let Ok(addr) = self.local_addr() {
            d.field("addr", &addr);
        }
        d.field("fd", &self.fd).finish()
    }
}
