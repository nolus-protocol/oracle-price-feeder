use std::fmt::{Display, Formatter, Result as FmtResult};

pub type VersionSegment = u16;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemVer {
    major: VersionSegment,
    minor: VersionSegment,
    patch: VersionSegment,
}

impl SemVer {
    pub const fn new(major: VersionSegment, minor: VersionSegment, patch: VersionSegment) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub const fn check_compatibility(&self, expected: Self) -> bool {
        self.major == expected.major
            && ((self.minor == expected.minor && self.patch >= expected.patch)
                || (self.major != 0 && self.minor > expected.minor))
    }
}

impl Display for SemVer {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_fmt(format_args!("{}.{}.{}", self.major, self.major, self.patch))
    }
}
