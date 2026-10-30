// `std::io` (W-SHIM): errors, the `Write` trait, and the standard
// output streams.
//
// Behavior follows rustc 1.99 std on Linux: `Error` Display and Debug
// texts per representation, the OS error kind table, `write_all`'s
// retry and `WriteZero` rules. The standard streams write through the
// runtime's C stdio streams, the same ones `print!` and `eprint!` use,
// so their output interleaves exactly as one std stream would.

use core::fmt;

/// `std::io::Result<T>`: a `Result` with `io::Error` as its error.
pub type Result<T> = core::result::Result<T, Error>;

/// The error type for I/O operations of `Read`, `Write`, and the
/// filesystem and process functions.
pub struct Error {
    repr: Repr,
}

/// How an `Error` was built. The variants match std's representations,
/// which decide the Display and Debug texts.
enum Repr {
    /// An OS error code (`from_raw_os_error`, failed syscalls).
    Os(i32),
    /// Only a kind (`From<ErrorKind>`).
    Simple(ErrorKind),
    /// A kind with a static message (errors std itself makes).
    SimpleMessage(ErrorKind, &'static str),
    /// A kind with a user payload (`Error::new`, `Error::other`). The
    /// payload is the text of a `&str` or `String`, the error types lust
    /// accepts here.
    Custom(ErrorKind, String),
    /// A kind with a payload of an adapter crate's own data-less error
    /// type (`__lust_adapter_error`): the payload's Display and Debug.
    CustomFmt(ErrorKind, PayloadFmt, PayloadFmt),
}

/// Prints one side (Display or Debug) of a `Repr::CustomFmt` payload.
type PayloadFmt = fn(&mut fmt::Formatter<'_>) -> fmt::Result;

/// Debug of a `Repr::CustomFmt` payload as a struct field.
struct PayloadDebug(PayloadFmt);

impl fmt::Debug for PayloadDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.0)(f)
    }
}


/// The payloads `Error::new` and `Error::other` take in lust: `&str`
/// and `String` (std takes anything convertible into a boxed error).
/// Sealed in a private module: user code can neither name nor
/// implement it.
mod custom {
    pub trait IntoCustomError {
        fn into_custom_text(self) -> String;
    }

    impl IntoCustomError for &str {
        fn into_custom_text(self) -> String {
            String::from(self)
        }
    }

    impl IntoCustomError for String {
        fn into_custom_text(self) -> String {
            self
        }
    }
}

use custom::IntoCustomError;

impl Error {
    /// Creates an error from a kind and a payload error.
    pub fn new<E: IntoCustomError>(kind: ErrorKind, error: E) -> Error {
        Error { repr: Repr::Custom(kind, error.into_custom_text()) }
    }

    /// Creates an error of kind `Other` from a payload error.
    pub fn other<E: IntoCustomError>(error: E) -> Error {
        Error::new(ErrorKind::Other, error)
    }

    /// For adapter crates only (user code that names it is L0104): std's
    /// `Error::new(kind, payload)` for an adapter's payload type that has no
    /// data (flate2's `CompressError`). `display` and `debug` print the
    /// payload, as std prints a `Custom` error through the boxed payload.
    /// Not generic on purpose: a generic inherent fn here slowed method
    /// lookup on io::Error in programs with unresolved names (parse-fuzz).
    #[doc(hidden)]
    pub fn __lust_adapter_error(kind: ErrorKind, display: PayloadFmt, debug: PayloadFmt) -> Error {
        Error { repr: Repr::CustomFmt(kind, display, debug) }
    }

    /// std's `const_error!(kind, message)` for std source outside this
    /// crate (the std splice): an error with a static message.
    #[doc(hidden)]
    pub fn __lust_adapter_simple_message(kind: ErrorKind, message: &'static str) -> Error {
        Error::simple_message(kind, message)
    }

    /// Creates an error from an OS error code.
    pub fn from_raw_os_error(code: i32) -> Error {
        Error { repr: Repr::Os(code) }
    }

    /// The OS error code, if this error came from the OS.
    pub fn raw_os_error(&self) -> Option<i32> {
        match self.repr {
            Repr::Os(code) => Some(code),
            _ => None,
        }
    }

    /// The general category of this error.
    pub fn kind(&self) -> ErrorKind {
        match self.repr {
            Repr::Os(code) => decode_error_kind(code),
            Repr::Simple(kind) => kind,
            Repr::SimpleMessage(kind, _) => kind,
            Repr::Custom(kind, _) => kind,
            Repr::CustomFmt(kind, _, _) => kind,
        }
    }

    /// An error with a static message, as std's `const_error!`.
    pub(crate) fn simple_message(kind: ErrorKind, message: &'static str) -> Error {
        Error { repr: Repr::SimpleMessage(kind, message) }
    }

    /// True for the kind `write_all` and `read_exact` retry on.
    pub(crate) fn is_interrupted(&self) -> bool {
        match self.repr {
            Repr::Os(code) => code == 4,
            Repr::Simple(kind) => kind == ErrorKind::Interrupted,
            Repr::SimpleMessage(kind, _) => kind == ErrorKind::Interrupted,
            Repr::Custom(kind, _) => kind == ErrorKind::Interrupted,
            Repr::CustomFmt(kind, _, _) => kind == ErrorKind::Interrupted,
        }
    }
}

/// The OS text of an error code, as std's `error_string` (strerror).
fn os_error_text(code: i32) -> String {
    let mut text = String::new();
    core::intrinsics::sys_os_error_text(code, &mut text);
    text
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { repr: Repr::Simple(kind) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Os(code) => {
                let detail = os_error_text(*code);
                write!(f, "{} (os error {})", detail, code)
            }
            Repr::Simple(kind) => f.write_str(kind.as_str()),
            Repr::SimpleMessage(_, message) => f.pad(message),
            Repr::Custom(_, text) => f.pad(text),
            Repr::CustomFmt(_, display, _) => display(f),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Os(code) => {
                let message = os_error_text(*code);
                f.debug_struct("Os").field("code", code).field("kind", &decode_error_kind(*code)).field("message", &message).finish()
            }
            Repr::Simple(kind) => f.debug_tuple("Kind").field(kind).finish(),
            Repr::SimpleMessage(kind, message) => f.debug_struct("Error").field("kind", kind).field("message", message).finish(),
            Repr::Custom(kind, text) => f.debug_struct("Custom").field("kind", kind).field("error", text).finish(),
            Repr::CustomFmt(kind, _, debug) => f.debug_struct("Custom").field("kind", kind).field("error", &PayloadDebug(*debug)).finish(),
        }
    }
}

/// A list specifying general categories of I/O error, in std's order
/// (the derived `Ord` and `Hash` read it). Variants std keeps unstable
/// carry a `__` prefix; their `Debug` text is std's name. std marks the
/// enum `#[non_exhaustive]`; the `__` variants give user matches the same
/// need for a wildcard arm.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    HostUnreachable,
    NetworkUnreachable,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    AddrNotAvailable,
    NetworkDown,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    NotADirectory,
    IsADirectory,
    DirectoryNotEmpty,
    ReadOnlyFilesystem,
    __FilesystemLoop,
    StaleNetworkFileHandle,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    StorageFull,
    NotSeekable,
    QuotaExceeded,
    FileTooLarge,
    ResourceBusy,
    ExecutableFileBusy,
    Deadlock,
    CrossesDevices,
    TooManyLinks,
    InvalidFilename,
    ArgumentListTooLong,
    Interrupted,
    Unsupported,
    UnexpectedEof,
    OutOfMemory,
    __InProgress,
    __TooManyOpenFiles,
    __InputOutputError,
    Other,
    __Uncategorized,
}

impl ErrorKind {
    /// The short description std's `Display` prints.
    fn as_str(&self) -> &'static str {
        match *self {
            ErrorKind::NotFound => "entity not found",
            ErrorKind::PermissionDenied => "permission denied",
            ErrorKind::ConnectionRefused => "connection refused",
            ErrorKind::ConnectionReset => "connection reset",
            ErrorKind::HostUnreachable => "host unreachable",
            ErrorKind::NetworkUnreachable => "network unreachable",
            ErrorKind::ConnectionAborted => "connection aborted",
            ErrorKind::NotConnected => "not connected",
            ErrorKind::AddrInUse => "address in use",
            ErrorKind::AddrNotAvailable => "address not available",
            ErrorKind::NetworkDown => "network down",
            ErrorKind::BrokenPipe => "broken pipe",
            ErrorKind::AlreadyExists => "entity already exists",
            ErrorKind::WouldBlock => "operation would block",
            ErrorKind::NotADirectory => "not a directory",
            ErrorKind::IsADirectory => "is a directory",
            ErrorKind::DirectoryNotEmpty => "directory not empty",
            ErrorKind::ReadOnlyFilesystem => "read-only filesystem or storage medium",
            ErrorKind::__FilesystemLoop => "filesystem loop or indirection limit (e.g. symlink loop)",
            ErrorKind::StaleNetworkFileHandle => "stale network file handle",
            ErrorKind::InvalidInput => "invalid input parameter",
            ErrorKind::InvalidData => "invalid data",
            ErrorKind::TimedOut => "timed out",
            ErrorKind::WriteZero => "write zero",
            ErrorKind::StorageFull => "no storage space",
            ErrorKind::NotSeekable => "seek on unseekable file",
            ErrorKind::QuotaExceeded => "quota exceeded",
            ErrorKind::FileTooLarge => "file too large",
            ErrorKind::ResourceBusy => "resource busy",
            ErrorKind::ExecutableFileBusy => "executable file busy",
            ErrorKind::Deadlock => "deadlock",
            ErrorKind::CrossesDevices => "cross-device link or rename",
            ErrorKind::TooManyLinks => "too many links",
            ErrorKind::InvalidFilename => "invalid filename",
            ErrorKind::ArgumentListTooLong => "argument list too long",
            ErrorKind::Interrupted => "operation interrupted",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::UnexpectedEof => "unexpected end of file",
            ErrorKind::OutOfMemory => "out of memory",
            ErrorKind::__InProgress => "in progress",
            ErrorKind::__TooManyOpenFiles => "too many open files",
            ErrorKind::__InputOutputError => "input/output error",
            ErrorKind::Other => "other error",
            ErrorKind::__Uncategorized => "uncategorized error",
        }
    }

    /// The variant name as std's derived `Debug` prints it.
    fn name(&self) -> &'static str {
        match *self {
            ErrorKind::NotFound => "NotFound",
            ErrorKind::PermissionDenied => "PermissionDenied",
            ErrorKind::ConnectionRefused => "ConnectionRefused",
            ErrorKind::ConnectionReset => "ConnectionReset",
            ErrorKind::HostUnreachable => "HostUnreachable",
            ErrorKind::NetworkUnreachable => "NetworkUnreachable",
            ErrorKind::ConnectionAborted => "ConnectionAborted",
            ErrorKind::NotConnected => "NotConnected",
            ErrorKind::AddrInUse => "AddrInUse",
            ErrorKind::AddrNotAvailable => "AddrNotAvailable",
            ErrorKind::NetworkDown => "NetworkDown",
            ErrorKind::BrokenPipe => "BrokenPipe",
            ErrorKind::AlreadyExists => "AlreadyExists",
            ErrorKind::WouldBlock => "WouldBlock",
            ErrorKind::NotADirectory => "NotADirectory",
            ErrorKind::IsADirectory => "IsADirectory",
            ErrorKind::DirectoryNotEmpty => "DirectoryNotEmpty",
            ErrorKind::ReadOnlyFilesystem => "ReadOnlyFilesystem",
            ErrorKind::__FilesystemLoop => "FilesystemLoop",
            ErrorKind::StaleNetworkFileHandle => "StaleNetworkFileHandle",
            ErrorKind::InvalidInput => "InvalidInput",
            ErrorKind::InvalidData => "InvalidData",
            ErrorKind::TimedOut => "TimedOut",
            ErrorKind::WriteZero => "WriteZero",
            ErrorKind::StorageFull => "StorageFull",
            ErrorKind::NotSeekable => "NotSeekable",
            ErrorKind::QuotaExceeded => "QuotaExceeded",
            ErrorKind::FileTooLarge => "FileTooLarge",
            ErrorKind::ResourceBusy => "ResourceBusy",
            ErrorKind::ExecutableFileBusy => "ExecutableFileBusy",
            ErrorKind::Deadlock => "Deadlock",
            ErrorKind::CrossesDevices => "CrossesDevices",
            ErrorKind::TooManyLinks => "TooManyLinks",
            ErrorKind::InvalidFilename => "InvalidFilename",
            ErrorKind::ArgumentListTooLong => "ArgumentListTooLong",
            ErrorKind::Interrupted => "Interrupted",
            ErrorKind::Unsupported => "Unsupported",
            ErrorKind::UnexpectedEof => "UnexpectedEof",
            ErrorKind::OutOfMemory => "OutOfMemory",
            ErrorKind::__InProgress => "InProgress",
            ErrorKind::__TooManyOpenFiles => "TooManyOpenFiles",
            ErrorKind::__InputOutputError => "InputOutputError",
            ErrorKind::Other => "Other",
            ErrorKind::__Uncategorized => "Uncategorized",
        }
    }
}

/// The kind of an OS error code, as std's `decode_error_kind` on Linux.
#[cfg(target_os = "linux")]
fn decode_error_kind(code: i32) -> ErrorKind {
    match code {
        1 => ErrorKind::PermissionDenied,
        2 => ErrorKind::NotFound,
        4 => ErrorKind::Interrupted,
        5 => ErrorKind::__InputOutputError,
        7 => ErrorKind::ArgumentListTooLong,
        11 => ErrorKind::WouldBlock,
        12 => ErrorKind::OutOfMemory,
        13 => ErrorKind::PermissionDenied,
        16 => ErrorKind::ResourceBusy,
        17 => ErrorKind::AlreadyExists,
        18 => ErrorKind::CrossesDevices,
        20 => ErrorKind::NotADirectory,
        21 => ErrorKind::IsADirectory,
        22 => ErrorKind::InvalidInput,
        23 => ErrorKind::__TooManyOpenFiles,
        24 => ErrorKind::__TooManyOpenFiles,
        26 => ErrorKind::ExecutableFileBusy,
        27 => ErrorKind::FileTooLarge,
        28 => ErrorKind::StorageFull,
        29 => ErrorKind::NotSeekable,
        30 => ErrorKind::ReadOnlyFilesystem,
        31 => ErrorKind::TooManyLinks,
        32 => ErrorKind::BrokenPipe,
        35 => ErrorKind::Deadlock,
        36 => ErrorKind::InvalidFilename,
        38 => ErrorKind::Unsupported,
        39 => ErrorKind::DirectoryNotEmpty,
        40 => ErrorKind::__FilesystemLoop,
        95 => ErrorKind::Unsupported,
        98 => ErrorKind::AddrInUse,
        99 => ErrorKind::AddrNotAvailable,
        100 => ErrorKind::NetworkDown,
        101 => ErrorKind::NetworkUnreachable,
        103 => ErrorKind::ConnectionAborted,
        104 => ErrorKind::ConnectionReset,
        107 => ErrorKind::NotConnected,
        110 => ErrorKind::TimedOut,
        111 => ErrorKind::ConnectionRefused,
        113 => ErrorKind::HostUnreachable,
        115 => ErrorKind::__InProgress,
        116 => ErrorKind::StaleNetworkFileHandle,
        122 => ErrorKind::QuotaExceeded,
        _ => ErrorKind::__Uncategorized,
    }
}

/// The kind of an OS error code, as std's `decode_error_kind` on
/// macOS: the same map by errno name, with Darwin's numbers
/// (EAGAIN is WouldBlock, as EWOULDBLOCK is EAGAIN there).
#[cfg(target_os = "macos")]
fn decode_error_kind(code: i32) -> ErrorKind {
    match code {
        1 => ErrorKind::PermissionDenied,
        2 => ErrorKind::NotFound,
        4 => ErrorKind::Interrupted,
        5 => ErrorKind::__InputOutputError,
        7 => ErrorKind::ArgumentListTooLong,
        11 => ErrorKind::Deadlock,
        12 => ErrorKind::OutOfMemory,
        13 => ErrorKind::PermissionDenied,
        16 => ErrorKind::ResourceBusy,
        17 => ErrorKind::AlreadyExists,
        18 => ErrorKind::CrossesDevices,
        20 => ErrorKind::NotADirectory,
        21 => ErrorKind::IsADirectory,
        22 => ErrorKind::InvalidInput,
        23 => ErrorKind::__TooManyOpenFiles,
        24 => ErrorKind::__TooManyOpenFiles,
        26 => ErrorKind::ExecutableFileBusy,
        27 => ErrorKind::FileTooLarge,
        28 => ErrorKind::StorageFull,
        29 => ErrorKind::NotSeekable,
        30 => ErrorKind::ReadOnlyFilesystem,
        31 => ErrorKind::TooManyLinks,
        32 => ErrorKind::BrokenPipe,
        35 => ErrorKind::WouldBlock,
        36 => ErrorKind::__InProgress,
        48 => ErrorKind::AddrInUse,
        49 => ErrorKind::AddrNotAvailable,
        50 => ErrorKind::NetworkDown,
        51 => ErrorKind::NetworkUnreachable,
        53 => ErrorKind::ConnectionAborted,
        54 => ErrorKind::ConnectionReset,
        57 => ErrorKind::NotConnected,
        60 => ErrorKind::TimedOut,
        61 => ErrorKind::ConnectionRefused,
        62 => ErrorKind::__FilesystemLoop,
        63 => ErrorKind::InvalidFilename,
        65 => ErrorKind::HostUnreachable,
        66 => ErrorKind::DirectoryNotEmpty,
        69 => ErrorKind::QuotaExceeded,
        70 => ErrorKind::StaleNetworkFileHandle,
        78 => ErrorKind::Unsupported,
        102 => ErrorKind::Unsupported,
        _ => ErrorKind::__Uncategorized,
    }
}

impl fmt::Debug for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A trait for objects which are byte-oriented sinks.
pub trait Write {
    /// Writes a buffer into this writer, returning how many bytes were
    /// written.
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    /// Flushes this output stream.
    fn flush(&mut self) -> Result<()>;

    /// Writes an entire buffer, retrying on `Interrupted`; a write of
    /// zero bytes is a `WriteZero` error.
    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        let mut rest = buf;
        while !rest.is_empty() {
            match self.write(rest) {
                Ok(0) => return Err(Error::simple_message(ErrorKind::WriteZero, "failed to write whole buffer")),
                Ok(n) => rest = &rest[n..], //@ core/src/io/write.rs:220:36
                Err(e) => {
                    if !e.is_interrupted() {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Writes a formatted string into this writer (`write!` and
    /// `writeln!`). lust formats the `fmt::Arguments` into one buffer
    /// and writes it with one `write_all`; for std's writers that is
    /// the same output as std's piece-by-piece writes. The compiler
    /// streams `write!` on `Stdout`, `Stderr`, and their locks piece
    /// by piece instead of calling this default, so a print inside a
    /// fmt impl stays in order there. A user type that relies on this
    /// default is rejected when a build instantiates it (L0641),
    /// since std would call its `write` once per piece.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        let bytes = core::intrinsics::sys_fmt_arguments_bytes(args);
        self.write_all(&bytes)
    }

    /// Creates a "by reference" adapter for this writer.
    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

impl Write for Vec<u8> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.extend_from_slice(buf);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl<W: Write + ?Sized> Write for Box<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        (**self).write_all(buf)
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> Result<()> {
        (**self).write_fmt(fmt)
    }
}

impl<W: Write + ?Sized> Write for &mut W {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        (**self).write_all(buf)
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> Result<()> {
        (**self).write_fmt(fmt)
    }
}

/// Writes `buf` to the process's stdout (fd 1) or stderr (fd 2)
/// through the runtime's stdio stream.
fn write_std(fd: i32, buf: &[u8]) -> Result<usize> {
    let n = core::intrinsics::sys_write_std(fd, buf);
    if n < 0 { Err(Error::from_raw_os_error((-n) as i32)) } else { Ok(n as usize) }
}

/// Flushes the runtime's stdio stream for fd 1 or 2.
fn flush_std(fd: i32) -> Result<()> {
    let code = core::intrinsics::sys_flush_std(fd);
    if code != 0 { Err(Error::from_raw_os_error(code)) } else { Ok(()) }
}

/// The unit value the stream locks borrow for `'static`.
static UNIT: () = ();

/// A handle to the global standard output stream of the current
/// process.
pub struct Stdout {
    _priv: (),
}

/// A locked reference to the `Stdout` handle.
pub struct StdoutLock<'a> {
    _handle: &'a (),
}

/// Constructs a new handle to the standard output of the current
/// process.
pub fn stdout() -> Stdout {
    Stdout { _priv: () }
}

impl Stdout {
    /// Locks this handle to the standard output stream.
    pub fn lock(&self) -> StdoutLock<'static> {
        StdoutLock { _handle: &UNIT }
    }
}

impl fmt::Debug for Stdout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stdout").finish_non_exhaustive()
    }
}

impl fmt::Debug for StdoutLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdoutLock").finish_non_exhaustive()
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        write_std(1, buf)
    }

    fn flush(&mut self) -> Result<()> {
        flush_std(1)
    }
}

impl Write for StdoutLock<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        write_std(1, buf)
    }

    fn flush(&mut self) -> Result<()> {
        flush_std(1)
    }
}

/// A handle to the standard error stream of a process.
pub struct Stderr {
    _priv: (),
}

/// A locked reference to the `Stderr` handle.
pub struct StderrLock<'a> {
    _handle: &'a (),
}

/// Constructs a new handle to the standard error of the current
/// process.
pub fn stderr() -> Stderr {
    Stderr { _priv: () }
}

impl Stderr {
    /// Locks this handle to the standard error stream.
    pub fn lock(&self) -> StderrLock<'static> {
        StderrLock { _handle: &UNIT }
    }
}

impl fmt::Debug for Stderr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stderr").finish_non_exhaustive()
    }
}

impl fmt::Debug for StderrLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StderrLock").finish_non_exhaustive()
    }
}

impl Write for Stderr {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        write_std(2, buf)
    }

    fn flush(&mut self) -> Result<()> {
        flush_std(2)
    }
}

impl Write for StderrLock<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        write_std(2, buf)
    }

    fn flush(&mut self) -> Result<()> {
        flush_std(2)
    }
}

/// A writer which will move data into the void.
pub struct Sink {
    _priv: (),
}

/// Creates an instance of a writer which will successfully consume all
/// data.
pub const fn sink() -> Sink {
    Sink { _priv: () }
}

impl fmt::Debug for Sink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Sink")
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Copies `src` into the start of `dst` (`copy_from_slice` for the
/// equal-length prefixes the readers here copy).
fn copy_bytes(dst: &mut [u8], src: &[u8]) {
    for i in 0..src.len() {
        dst[i] = src[i];
    }
}

/// std's error for bytes that are not valid UTF-8 (`read_to_string`,
/// `read_line`).
pub(crate) fn invalid_utf8_error() -> Error {
    Error::simple_message(ErrorKind::InvalidData, "stream did not contain valid UTF-8")
}

/// Finishes a read that appended to a `String`'s bytes from `start`:
/// the string (truncated back to `start` when the new bytes are not
/// valid UTF-8, as std's `append_to_string` guard does) and whether
/// the new bytes were valid.
fn validate_appended(mut bytes: Vec<u8>, start: usize) -> (String, bool) {
    let valid = core::intrinsics::sys_utf8_valid_up_to(&bytes[start..]) == bytes.len() - start;
    if !valid {
        bytes.truncate(start);
    }
    (core::intrinsics::sys_string_from_utf8_unchecked(bytes), valid)
}

/// `Read::read_to_end`'s default, with std's buffer growth (std's
/// `default_read_to_end`): while the spare capacity is below 32,
/// read into a 32-byte stack probe and append, so a short stream
/// does not grow the Vec for a read that only finds the end; once an
/// append does grow the Vec, read into the spare capacity instead,
/// reserving 32 more whenever the spare drops below 32. The spare is
/// staged through a fresh Vec because safe code cannot write into a
/// Vec's spare capacity; the reader sees the same buffer sizes and
/// the Vec grows the same way.
pub(crate) fn read_to_end_default<R: Read + ?Sized>(r: &mut R, buf: &mut Vec<u8>) -> Result<usize> {
    let start = buf.len();
    let mut probe_ok = true;
    loop {
        if buf.capacity() - buf.len() < 32 {
            if probe_ok {
                let mut probe = [0u8; 32];
                match r.read(&mut probe) {
                    Ok(0) => return Ok(buf.len() - start),
                    Ok(n) => {
                        let cap_before = buf.capacity();
                        buf.extend_from_slice(&probe[..n]); //@ alloc/src/io/read.rs:878:57
                        if buf.capacity() != cap_before {
                            probe_ok = false;
                        }
                    }
                    Err(e) => {
                        if !e.is_interrupted() {
                            return Err(e);
                        }
                    }
                }
                continue;
            }
            buf.reserve(32);
        }
        let spare = buf.capacity() - buf.len();
        let mut chunk: Vec<u8> = vec![0u8; spare];
        match r.read(&mut chunk) {
            Ok(0) => return Ok(buf.len() - start),
            Ok(n) => {
                // std reads into the spare capacity through a cursor,
                // whose `advance_checked` asserts the count.
                let init_unfilled = chunk.len();
                assert!(n <= init_unfilled); //@ core/src/io/borrowed_buf.rs:287:9
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => {
                if !e.is_interrupted() {
                    return Err(e);
                }
            }
        }
    }
}

/// A trait for objects which are byte-oriented sources.
pub trait Read {
    /// Reads some bytes into the buffer, returning how many were read.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    /// Reads the exact number of bytes needed to fill the buffer; a
    /// short read is `UnexpectedEof`.
    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => return Err(Error::simple_message(ErrorKind::UnexpectedEof, "failed to fill whole buffer")),
                Ok(n) => {
                    let rest = buf;
                    buf = &mut rest[n..]; //@ alloc/src/io/read.rs:1031:31
                }
                Err(e) => {
                    if !e.is_interrupted() {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Reads all bytes until the end of the stream, appending them to
    /// the buffer.
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        read_to_end_default(self, buf)
    }

    /// Reads all bytes until the end of the stream as text, appending
    /// them to the string. Bytes that are not valid UTF-8 make the
    /// string keep only what it held before; on a read error the
    /// valid bytes read so far stay.
    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        let start = buf.len();
        let mut bytes = core::mem::replace(buf, String::new()).into_bytes();
        let res = self.read_to_end(&mut bytes);
        let (s, valid) = validate_appended(bytes, start);
        *buf = s;
        match res {
            Err(e) => Err(e),
            Ok(n) => {
                if valid {
                    Ok(n)
                } else {
                    Err(invalid_utf8_error())
                }
            }
        }
    }

    /// Creates a "by reference" adapter for this reader.
    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

/// std 1.99's `io::copy` on the path it takes when neither side is a
/// type std specializes (alloc/src/io/copy/generic.rs
/// `stack_buffer_copy`): an 8 KiB buffer, `read` into it until 0 (an
/// `Interrupted` error is retried), then `write_all` of each chunk. A
/// `Read` impl's `read` sees the buffer as std's default `read_buf`
/// leaves it: zeroed once, then holding the earlier bytes. The calls std
/// specializes (in-memory and buffered readers and writers, two OS
/// handles) reject in check/platform.rs `io_copy_diag`.
pub fn copy<R: Read + ?Sized, W: Write + ?Sized>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut buf = [0u8; 8192];
    let mut len: u64 = 0;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                if e.is_interrupted() {
                    continue;
                }
                return Err(e);
            }
        };
        if n > buf.len() {
            core::intrinsics::panic_read_buf_overrun();
            return Ok(len);
        }
        if n == 0 {
            return Ok(len);
        }
        len += n as u64;
        writer.write_all(&buf[..n])?;
    }
}

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let cur: &[u8] = *self;
        let n = buf.len().min(cur.len());
        copy_bytes(&mut buf[..n], &cur[..n]);
        *self = &cur[n..];
        Ok(n)
    }

    /// As std: a short `read_exact` consumes the rest of the slice but
    /// leaves the buffer untouched.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let cur: &[u8] = *self;
        let want = buf.len();
        if want <= cur.len() {
            copy_bytes(buf, &cur[..want]);
            *self = &cur[want..];
            Ok(())
        } else {
            *self = &cur[cur.len()..];
            Err(Error::simple_message(ErrorKind::UnexpectedEof, "failed to fill whole buffer"))
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let cur: &[u8] = *self;
        buf.extend_from_slice(cur);
        *self = &cur[cur.len()..];
        Ok(cur.len())
    }

    /// As std: the whole slice is checked first, so invalid UTF-8
    /// consumes nothing.
    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        let cur: &[u8] = *self;
        if core::intrinsics::sys_utf8_valid_up_to(cur) != cur.len() {
            return Err(invalid_utf8_error());
        }
        buf.push_str(core::intrinsics::sys_bytes_as_str(cur));
        *self = &cur[cur.len()..];
        Ok(cur.len())
    }
}

impl<R: Read + ?Sized> Read for &mut R {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        (**self).read_exact(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        (**self).read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        (**self).read_to_string(buf)
    }
}

impl<R: Read + ?Sized> Read for Box<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        (**self).read_exact(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        (**self).read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        (**self).read_to_string(buf)
    }
}

/// `BufRead::read_until`'s default over `fill_buf` and `consume`.
fn read_until_default<B: BufRead + ?Sized>(r: &mut B, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
    let mut read = 0;
    loop {
        let (used, found) = {
            let avail = match r.fill_buf() {
                Ok(a) => a,
                Err(e) => {
                    if e.is_interrupted() {
                        continue;
                    }
                    return Err(e);
                }
            };
            if avail.is_empty() {
                return Ok(read);
            }
            let mut at: Option<usize> = None;
            for i in 0..avail.len() {
                if avail[i] == byte {
                    at = Some(i);
                    break;
                }
            }
            match at {
                Some(i) => {
                    buf.extend_from_slice(&avail[..i + 1]);
                    (i + 1, true)
                }
                None => {
                    buf.extend_from_slice(avail);
                    (avail.len(), false)
                }
            }
        };
        r.consume(used);
        read += used;
        if found {
            return Ok(read);
        }
    }
}

/// A buffered reader: `fill_buf` shows the buffered bytes, `consume`
/// marks some of them read.
pub trait BufRead: Read {
    /// The buffered bytes, reading more from the source when empty.
    fn fill_buf(&mut self) -> Result<&[u8]>;

    /// Marks `amt` buffered bytes as read.
    fn consume(&mut self, amt: usize);

    /// Reads bytes into the buffer until the delimiter or the end of
    /// the stream, returning how many were read (the delimiter is
    /// included and consumed).
    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
        read_until_default(self, byte, buf)
    }

    /// Reads one line (through the newline) as text, appending it to
    /// the string. Bytes that are not valid UTF-8 are consumed but
    /// not appended.
    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        let start = buf.len();
        let mut bytes = core::mem::replace(buf, String::new()).into_bytes();
        let res = self.read_until(b'\n', &mut bytes);
        let (s, valid) = validate_appended(bytes, start);
        *buf = s;
        match res {
            Err(e) => Err(e),
            Ok(n) => {
                if valid {
                    Ok(n)
                } else {
                    Err(invalid_utf8_error())
                }
            }
        }
    }

    /// An iterator over the lines of this reader (line ends removed).
    fn lines(self) -> Lines<Self>
    where
        Self: Sized,
    {
        Lines { buf: self }
    }

    /// An iterator over the byte records of this reader, split at the
    /// delimiter (which is removed).
    fn split(self, byte: u8) -> Split<Self>
    where
        Self: Sized,
    {
        Split { buf: self, delim: byte }
    }
}

impl BufRead for &[u8] {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        Ok(*self)
    }

    fn consume(&mut self, amt: usize) {
        let cur: &[u8] = *self;
        *self = &cur[amt..]; //@ alloc/src/io/impls.rs:389:22
    }
}

impl<B: BufRead + ?Sized> BufRead for &mut B {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        (**self).fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        (**self).consume(amt)
    }

    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
        (**self).read_until(byte, buf)
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        (**self).read_line(buf)
    }
}

impl<B: BufRead + ?Sized> BufRead for Box<B> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        (**self).fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        (**self).consume(amt)
    }

    fn read_until(&mut self, byte: u8, buf: &mut Vec<u8>) -> Result<usize> {
        (**self).read_until(byte, buf)
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        (**self).read_line(buf)
    }
}

/// An iterator over the lines of a buffered reader, from
/// `BufRead::lines`.
pub struct Lines<B> {
    buf: B,
}

impl<B: BufRead> Iterator for Lines<B> {
    type Item = Result<String>;

    fn next(&mut self) -> Option<Result<String>> {
        let mut line = String::new();
        match self.buf.read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => {
                if line.ends_with("\n") {
                    let _ = line.pop();
                    if line.ends_with("\r") {
                        let _ = line.pop();
                    }
                }
                Some(Ok(line))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl<B: BufRead + fmt::Debug> fmt::Debug for Lines<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lines").field("buf", &self.buf).finish()
    }
}

/// An iterator over the byte records of a buffered reader, from
/// `BufRead::split`.
pub struct Split<B> {
    buf: B,
    delim: u8,
}

impl<B: BufRead> Iterator for Split<B> {
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Result<Vec<u8>>> {
        let mut v: Vec<u8> = Vec::new();
        match self.buf.read_until(self.delim, &mut v) {
            Ok(0) => None,
            Ok(_) => {
                if v.last() == Some(&self.delim) {
                    let _ = v.pop();
                }
                Some(Ok(v))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl<B: BufRead + fmt::Debug> fmt::Debug for Split<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Split").field("buf", &self.buf).field("delim", &self.delim).finish()
    }
}

/// The `used/capacity` text std's buffer Debug prints inside
/// `BufReader` and `BufWriter`.
struct BufferUsed {
    used: usize,
    cap: usize,
}

impl fmt::Debug for BufferUsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.used, self.cap)
    }
}

/// A buffered reader over another reader (`fill_buf` shows the
/// buffered bytes).
pub struct BufReader<R> {
    inner: R,
    /// The buffer storage; its length is the capacity.
    buf: Vec<u8>,
    /// The first unread byte in `buf`.
    pos: usize,
    /// The end of the filled bytes in `buf`.
    filled: usize,
}

impl<R: Read> BufReader<R> {
    /// Creates a buffered reader with a default capacity of 8192.
    pub fn new(inner: R) -> BufReader<R> {
        BufReader::with_capacity(8192, inner)
    }

    /// Creates a buffered reader with at least the given capacity.
    pub fn with_capacity(capacity: usize, inner: R) -> BufReader<R> {
        BufReader { inner, buf: vec![0u8; capacity], pos: 0, filled: 0 }
    }

    /// The inner reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// The inner reader, mutably. Buffered bytes stay buffered.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// The inner reader; buffered bytes are lost.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// The buffered bytes not yet read. Call `fill_buf` to fill first.
    pub fn buffer(&self) -> &[u8] {
        &self.buf[self.pos..self.filled]
    }

    /// The buffer's capacity.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.len() >= self.buf.len() && self.pos >= self.filled {
            self.pos = 0;
            self.filled = 0;
            return self.inner.read(buf);
        }
        let n = {
            let mut rem = self.fill_buf()?;
            rem.read(buf)?
        };
        self.consume(n);
        Ok(n)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let buffered = self.filled - self.pos;
        buf.extend_from_slice(&self.buf[self.pos..self.filled]);
        self.consume(buffered);
        match self.inner.read_to_end(buf) {
            Ok(n) => Ok(buffered + n),
            Err(e) => Err(e),
        }
    }

    /// As std's specialization for `BufReader`: into an empty string
    /// it reads like the `Read` default (valid bytes read before a
    /// read error stay); into a non-empty string it reads into a
    /// fresh buffer first and appends only on success, so any error
    /// leaves the string unchanged.
    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        if buf.is_empty() {
            let mut bytes = core::mem::replace(buf, String::new()).into_bytes();
            let res = self.read_to_end(&mut bytes);
            let (s, valid) = validate_appended(bytes, 0);
            *buf = s;
            return match res {
                Err(e) => Err(e),
                Ok(n) => {
                    if valid {
                        Ok(n)
                    } else {
                        Err(invalid_utf8_error())
                    }
                }
            };
        }
        let mut bytes: Vec<u8> = Vec::new();
        let n = match self.read_to_end(&mut bytes) {
            Ok(n) => n,
            Err(e) => return Err(e),
        };
        if core::intrinsics::sys_utf8_valid_up_to(&bytes) != bytes.len() {
            return Err(invalid_utf8_error());
        }
        buf.push_str(core::intrinsics::sys_bytes_as_str(&bytes));
        Ok(n)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.pos >= self.filled {
            let n = self.inner.read(&mut self.buf)?;
            // std reads through a cursor, whose `advance_checked` asserts
            // the count.
            let init_unfilled = self.buf.len();
            assert!(n <= init_unfilled); //@ core/src/io/borrowed_buf.rs:287:9
            self.filled = n;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.filled])
    }

    fn consume(&mut self, amt: usize) {
        let next = self.pos + amt;
        self.pos = if next <= self.filled { next } else { self.filled };
    }
}

impl<R: Read + fmt::Debug> fmt::Debug for BufReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufReader")
            .field("reader", &self.inner)
            .field("buffer", &BufferUsed { used: self.filled - self.pos, cap: self.buf.len() })
            .finish()
    }
}

/// A buffered writer over another writer. Dropping it attempts to
/// write the buffered bytes, as std does.
pub struct BufWriter<W: Write> {
    inner: W,
    /// The buffered bytes; its Vec capacity is the buffer capacity.
    buf: Vec<u8>,
}

impl<W: Write> BufWriter<W> {
    /// Creates a buffered writer with a default capacity of 8192.
    pub fn new(inner: W) -> BufWriter<W> {
        BufWriter::with_capacity(8192, inner)
    }

    /// Creates a buffered writer with at least the given capacity.
    pub fn with_capacity(capacity: usize, inner: W) -> BufWriter<W> {
        BufWriter { inner, buf: Vec::with_capacity(capacity) }
    }

    /// The inner writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// The inner writer, mutably. Buffered bytes stay buffered.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// The buffered bytes not yet written.
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// The buffer's capacity.
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Drops the first `n` buffered bytes (they were written). A writer
    /// that reported more bytes than it was given panics here, as std's
    /// `BufGuard` drop does in its `copy_within(written.., 0)`.
    fn drop_prefix(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let len = self.buf.len();
        if n > len {
            panic!("range start index {} out of range for slice of length {}", n, len); //@ core/src/slice/index.rs:1020:55
        }
        for i in n..len {
            self.buf[i - n] = self.buf[i];
        }
        self.buf.truncate(len - n);
    }

    /// Writes the buffered bytes to the inner writer; on an error
    /// the bytes not written stay buffered.
    fn flush_buf(&mut self) -> Result<()> {
        let mut written = 0;
        while written < self.buf.len() {
            match self.inner.write(&self.buf[written..]) {
                Ok(0) => {
                    self.drop_prefix(written);
                    return Err(Error::simple_message(ErrorKind::WriteZero, "failed to write the buffered data"));
                }
                Ok(n) => written += n,
                Err(e) => {
                    if e.is_interrupted() {
                        continue;
                    }
                    self.drop_prefix(written);
                    return Err(e);
                }
            }
        }
        self.drop_prefix(written);
        Ok(())
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if buf.len() > self.buf.capacity() - self.buf.len() {
            self.flush_buf()?;
        }
        if buf.len() >= self.buf.capacity() {
            self.inner.write(buf)
        } else {
            self.buf.extend_from_slice(buf);
            Ok(buf.len())
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        if buf.len() > self.buf.capacity() - self.buf.len() {
            self.flush_buf()?;
        }
        if buf.len() >= self.buf.capacity() {
            self.inner.write_all(buf)
        } else {
            self.buf.extend_from_slice(buf);
            Ok(())
        }
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        let bytes = core::intrinsics::sys_fmt_arguments_bytes(args);
        self.write_all(&bytes)
    }

    fn flush(&mut self) -> Result<()> {
        self.flush_buf()?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for BufWriter<W> {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            let _ = self.flush_buf();
        }
    }
}

impl<W: Write + fmt::Debug> fmt::Debug for BufWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufWriter")
            .field("writer", &self.inner)
            .field("buffer", &BufferUsed { used: self.buf.len(), cap: self.buf.capacity() })
            .finish()
    }
}

/// A seek position: from the start, the end, or the current
/// position of a stream.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

/// A trait for objects which can seek within a stream of bytes.
pub trait Seek {
    /// Seeks to a position, returning the new position from the
    /// start of the stream.
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;

    /// Seeks back to the start of the stream.
    fn rewind(&mut self) -> Result<()> {
        match self.seek(SeekFrom::Start(0)) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// The current position from the start of the stream.
    fn stream_position(&mut self) -> Result<u64> {
        self.seek(SeekFrom::Current(0))
    }

    /// Seeks by a relative offset.
    fn seek_relative(&mut self, offset: i64) -> Result<()> {
        match self.seek(SeekFrom::Current(offset)) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl<T: Seek + ?Sized> Seek for &mut T {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        (**self).seek(pos)
    }

    fn rewind(&mut self) -> Result<()> {
        (**self).rewind()
    }

    fn stream_position(&mut self) -> Result<u64> {
        (**self).stream_position()
    }
}

/// One seek on a file descriptor (for `File`).
pub(crate) fn seek_fd(fd: i32, pos: SeekFrom) -> Result<u64> {
    let (off, whence) = match pos {
        SeekFrom::Start(n) => (n as i64, 0),
        SeekFrom::End(n) => (n, 2),
        SeekFrom::Current(n) => (n, 1),
    };
    let v = core::intrinsics::sys_fd_seek(fd, off, whence);
    if v < 0 {
        Err(Error::from_raw_os_error((-v) as i32))
    } else {
        Ok(v as u64)
    }
}

/// One read from a file descriptor into the buffer, staged through a
/// Vec (for `File` and the process pipes).
pub(crate) fn read_fd(fd: i32, buf: &mut [u8]) -> Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut staged: Vec<u8> = Vec::new();
    let n = core::intrinsics::sys_fd_read(fd, buf.len() as u64, &mut staged);
    if n < 0 {
        Err(Error::from_raw_os_error((-n) as i32))
    } else {
        copy_bytes(&mut buf[..n as usize], &staged);
        Ok(n as usize)
    }
}

/// One write of the buffer to a file descriptor (for `File` and the
/// process pipes).
pub(crate) fn write_fd(fd: i32, buf: &[u8]) -> Result<usize> {
    let n = core::intrinsics::sys_fd_write(fd, buf);
    if n < 0 { Err(Error::from_raw_os_error((-n) as i32)) } else { Ok(n as usize) }
}

/// One read from stdin through the runtime's process-wide buffer.
fn read_stdin(buf: &mut [u8]) -> Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut staged: Vec<u8> = Vec::new();
    let n = core::intrinsics::sys_stdin_read(buf.len() as u64, &mut staged);
    if n < 0 {
        Err(Error::from_raw_os_error((-n) as i32))
    } else {
        copy_bytes(&mut buf[..n as usize], &staged);
        Ok(n as usize)
    }
}

/// The private supertrait that seals `IsTerminal`, as std's.
pub(crate) mod sealed_terminal {
    pub trait Sealed {}
    impl Sealed for crate::fs::File {}
    impl Sealed for crate::os::fd::OwnedFd {}
    impl Sealed for crate::os::fd::BorrowedFd<'_> {}
    impl Sealed for super::Stdin {}
    impl Sealed for super::StdinLock<'_> {}
    impl Sealed for super::Stdout {}
    impl Sealed for super::StdoutLock<'_> {}
    impl Sealed for super::Stderr {}
    impl Sealed for super::StderrLock<'_> {}
}

/// Whether a descriptor refers to a terminal (`isatty`, as std on
/// Linux). Sealed: std's types only.
pub trait IsTerminal: sealed_terminal::Sealed {
    /// True when the descriptor is a terminal.
    fn is_terminal(&self) -> bool;
}

/// `isatty` of a raw fd, for the `IsTerminal` impls.
pub(crate) fn fd_is_terminal(fd: i32) -> bool {
    core::intrinsics::sys_fd_isatty(fd) != 0
}

impl IsTerminal for Stdin {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(0)
    }
}

impl IsTerminal for StdinLock<'_> {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(0)
    }
}

impl IsTerminal for Stdout {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(1)
    }
}

impl IsTerminal for StdoutLock<'_> {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(1)
    }
}

impl IsTerminal for Stderr {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(2)
    }
}

impl IsTerminal for StderrLock<'_> {
    fn is_terminal(&self) -> bool {
        fd_is_terminal(2)
    }
}

/// A handle to the global standard input stream of the current
/// process. Its buffer lives in the runtime, so bytes one handle
/// buffers are the next bytes every handle reads, as in std.
pub struct Stdin {
    _priv: (),
}

/// A locked reference to the `Stdin` handle. Shares the runtime's
/// stdin buffer; `snapped` is the copy of the buffer `fill_buf`
/// returned and `pos` the part of it not yet consumed.
pub struct StdinLock<'a> {
    _handle: &'a (),
    snapped: Vec<u8>,
    pos: usize,
}

/// Constructs a new handle to the standard input of the current
/// process.
pub fn stdin() -> Stdin {
    Stdin { _priv: () }
}

impl Stdin {
    /// Locks this handle to the standard input stream.
    pub fn lock(&self) -> StdinLock<'static> {
        StdinLock { _handle: &UNIT, snapped: Vec::new(), pos: 0 }
    }

    /// Reads one line from standard input into the buffer (the lock
    /// is taken for the call, as in std).
    pub fn read_line(&self, buf: &mut String) -> Result<usize> {
        self.lock().read_line(buf)
    }

    /// An iterator over the lines of standard input (the lock is
    /// taken for the iterator's life, as in std).
    pub fn lines(&self) -> Lines<StdinLock<'static>> {
        self.lock().lines()
    }
}

impl<'a> StdinLock<'a> {
    /// An iterator over the lines of standard input.
    pub fn lines(self) -> Lines<StdinLock<'a>> {
        Lines { buf: self }
    }
}

impl fmt::Debug for Stdin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stdin").finish_non_exhaustive()
    }
}

impl fmt::Debug for StdinLock<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdinLock").finish_non_exhaustive()
    }
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_stdin(buf)
    }
}

impl Read for &Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_stdin(buf)
    }
}

impl Read for StdinLock<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.pos < self.snapped.len() {
            let n = {
                let mut rem = &self.snapped[self.pos..];
                rem.read(buf)?
            };
            self.consume(n);
            Ok(n)
        } else {
            read_stdin(buf)
        }
    }
}

impl BufRead for StdinLock<'_> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.pos >= self.snapped.len() {
            self.snapped.clear();
            let n = core::intrinsics::sys_stdin_fill(&mut self.snapped);
            if n < 0 {
                return Err(Error::from_raw_os_error((-n) as i32));
            }
            self.pos = 0;
        }
        Ok(&self.snapped[self.pos..])
    }

    fn consume(&mut self, amt: usize) {
        let avail = self.snapped.len() - self.pos;
        let n = amt.min(avail);
        self.pos += n;
        core::intrinsics::sys_stdin_consume(n as u64);
    }
}
