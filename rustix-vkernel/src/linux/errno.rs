use core::fmt;

pub const MAX_ERRNO: i32 = 4095;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Errno(i32);

pub type Result<T> = core::result::Result<T, Errno>;

impl Errno {
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const ESRCH: Self = Self(3);
    pub const EINTR: Self = Self(4);
    pub const EIO: Self = Self(5);
    pub const ENXIO: Self = Self(6);
    pub const E2BIG: Self = Self(7);
    pub const ENOEXEC: Self = Self(8);
    pub const EBADF: Self = Self(9);
    pub const ECHILD: Self = Self(10);
    pub const EAGAIN: Self = Self(11);
    pub const ENOMEM: Self = Self(12);
    pub const EACCES: Self = Self(13);
    pub const EFAULT: Self = Self(14);
    pub const ENOTBLK: Self = Self(15);
    pub const EBUSY: Self = Self(16);
    pub const EEXIST: Self = Self(17);
    pub const EXDEV: Self = Self(18);
    pub const ENODEV: Self = Self(19);
    pub const ENOTDIR: Self = Self(20);
    pub const EISDIR: Self = Self(21);
    pub const EINVAL: Self = Self(22);
    pub const ENFILE: Self = Self(23);
    pub const EMFILE: Self = Self(24);
    pub const ENOTTY: Self = Self(25);
    pub const ETXTBSY: Self = Self(26);
    pub const EFBIG: Self = Self(27);
    pub const ENOSPC: Self = Self(28);
    pub const ESPIPE: Self = Self(29);
    pub const EROFS: Self = Self(30);
    pub const EMLINK: Self = Self(31);
    pub const EPIPE: Self = Self(32);
    pub const EDOM: Self = Self(33);
    pub const ERANGE: Self = Self(34);
    pub const EDEADLK: Self = Self(35);
    pub const ENAMETOOLONG: Self = Self(36);
    pub const ENOLCK: Self = Self(37);
    pub const ENOSYS: Self = Self(38);
    pub const ENOTEMPTY: Self = Self(39);
    pub const ELOOP: Self = Self(40);
    pub const ENOMSG: Self = Self(42);
    pub const EIDRM: Self = Self(43);
    pub const ECHRNG: Self = Self(44);
    pub const EL2NSYNC: Self = Self(45);
    pub const EL3HLT: Self = Self(46);
    pub const EL3RST: Self = Self(47);
    pub const ELNRNG: Self = Self(48);
    pub const EUNATCH: Self = Self(49);
    pub const ENOCSI: Self = Self(50);
    pub const EL2HLT: Self = Self(51);
    pub const EBADE: Self = Self(52);
    pub const EBADR: Self = Self(53);
    pub const EXFULL: Self = Self(54);
    pub const ENOANO: Self = Self(55);
    pub const EBADRQC: Self = Self(56);
    pub const EBADSLT: Self = Self(57);
    pub const EBFONT: Self = Self(59);
    pub const ENOSTR: Self = Self(60);
    pub const ENODATA: Self = Self(61);
    pub const ETIME: Self = Self(62);
    pub const ENOSR: Self = Self(63);
    pub const ENONET: Self = Self(64);
    pub const ENOPKG: Self = Self(65);
    pub const EREMOTE: Self = Self(66);
    pub const ENOLINK: Self = Self(67);
    pub const EADV: Self = Self(68);
    pub const ESRMNT: Self = Self(69);
    pub const ECOMM: Self = Self(70);
    pub const EPROTO: Self = Self(71);
    pub const EMULTIHOP: Self = Self(72);
    pub const EDOTDOT: Self = Self(73);
    pub const EBADMSG: Self = Self(74);
    pub const EOVERFLOW: Self = Self(75);
    pub const ENOTUNIQ: Self = Self(76);
    pub const EBADFD: Self = Self(77);
    pub const EREMCHG: Self = Self(78);
    pub const ELIBACC: Self = Self(79);
    pub const ELIBBAD: Self = Self(80);
    pub const ELIBSCN: Self = Self(81);
    pub const ELIBMAX: Self = Self(82);
    pub const ELIBEXEC: Self = Self(83);
    pub const EILSEQ: Self = Self(84);
    pub const ERESTART: Self = Self(85);
    pub const ESTRPIPE: Self = Self(86);
    pub const EUSERS: Self = Self(87);
    pub const ENOTSOCK: Self = Self(88);
    pub const EDESTADDRREQ: Self = Self(89);
    pub const EMSGSIZE: Self = Self(90);
    pub const EPROTOTYPE: Self = Self(91);
    pub const ENOPROTOOPT: Self = Self(92);
    pub const EPROTONOSUPPORT: Self = Self(93);
    pub const ESOCKTNOSUPPORT: Self = Self(94);
    pub const EOPNOTSUPP: Self = Self(95);
    pub const EPFNOSUPPORT: Self = Self(96);
    pub const EAFNOSUPPORT: Self = Self(97);
    pub const EADDRINUSE: Self = Self(98);
    pub const EADDRNOTAVAIL: Self = Self(99);
    pub const ENETDOWN: Self = Self(100);
    pub const ENETUNREACH: Self = Self(101);
    pub const ENETRESET: Self = Self(102);
    pub const ECONNABORTED: Self = Self(103);
    pub const ECONNRESET: Self = Self(104);
    pub const ENOBUFS: Self = Self(105);
    pub const EISCONN: Self = Self(106);
    pub const ENOTCONN: Self = Self(107);
    pub const ESHUTDOWN: Self = Self(108);
    pub const ETOOMANYREFS: Self = Self(109);
    pub const ETIMEDOUT: Self = Self(110);
    pub const ECONNREFUSED: Self = Self(111);
    pub const EHOSTDOWN: Self = Self(112);
    pub const EHOSTUNREACH: Self = Self(113);
    pub const EALREADY: Self = Self(114);
    pub const EINPROGRESS: Self = Self(115);
    pub const ESTALE: Self = Self(116);
    pub const EUCLEAN: Self = Self(117);
    pub const ENOTNAM: Self = Self(118);
    pub const ENAVAIL: Self = Self(119);
    pub const EISNAM: Self = Self(120);
    pub const EREMOTEIO: Self = Self(121);
    pub const EDQUOT: Self = Self(122);
    pub const ENOMEDIUM: Self = Self(123);
    pub const EMEDIUMTYPE: Self = Self(124);
    pub const ECANCELED: Self = Self(125);
    pub const ENOKEY: Self = Self(126);
    pub const EKEYEXPIRED: Self = Self(127);
    pub const EKEYREVOKED: Self = Self(128);
    pub const EKEYREJECTED: Self = Self(129);
    pub const EOWNERDEAD: Self = Self(130);
    pub const ENOTRECOVERABLE: Self = Self(131);
    pub const ERFKILL: Self = Self(132);
    pub const EHWPOISON: Self = Self(133);

    pub const EWOULDBLOCK: Self = Self::EAGAIN;
    pub const EDEADLOCK: Self = Self::EDEADLK;
    pub const ENOTSUP: Self = Self::EOPNOTSUPP;

    pub const fn from_code(code: i32) -> Option<Self> {
        if code > 0 && code <= MAX_ERRNO {
            Some(Self(code))
        } else {
            None
        }
    }

    pub const fn code(self) -> i32 {
        self.0
    }

    pub const fn return_value(self) -> u64 {
        (-(self.0 as i64)) as u64
    }

    pub const fn message(self) -> &'static str {
        match self.0 {
            1 => "operation not permitted",
            2 => "no such file or directory",
            8 => "exec format error",
            9 => "bad file descriptor",
            10 => "no child processes",
            11 => "resource temporarily unavailable",
            12 => "cannot allocate memory",
            13 => "permission denied",
            14 => "bad address",
            17 => "file exists",
            20 => "not a directory",
            21 => "is a directory",
            22 => "invalid argument",
            27 => "file too large",
            28 => "no space left on device",
            30 => "read-only file system",
            36 => "file name too long",
            38 => "function not implemented",
            75 => "value too large for defined data type",
            90 => "message too long",
            95 => "operation not supported",
            _ => "Linux error",
        }
    }
}

#[allow(non_upper_case_globals)]
impl Errno {
    pub const NoEntry: Self = Self::ENOENT;
    pub const NoMemory: Self = Self::ENOMEM;
    pub const Access: Self = Self::EACCES;
    pub const Exists: Self = Self::EEXIST;
    pub const NotDirectory: Self = Self::ENOTDIR;
    pub const IsDirectory: Self = Self::EISDIR;
    pub const Invalid: Self = Self::EINVAL;
    pub const FileTooLarge: Self = Self::EFBIG;
    pub const NoSpace: Self = Self::ENOSPC;
    pub const ReadOnlyFilesystem: Self = Self::EROFS;
    pub const NameTooLong: Self = Self::ENAMETOOLONG;
}

impl fmt::Display for Errno {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}
