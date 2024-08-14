use serde::Deserialize;

pub use self::admin::Admin;

pub mod admin;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize)]
pub struct SemVer {
    major: VersionSegment,
    minor: VersionSegment,
    patch: VersionSegment,
}

impl SemVer {
    #[must_use]
    pub const fn new(
        major: VersionSegment,
        minor: VersionSegment,
        patch: VersionSegment,
    ) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub const fn check_compatibility(
        &self,
        compatible_version: SemVer,
    ) -> Compatibility {
        if self.major == compatible_version.major
            && ((self.minor == compatible_version.minor
                && self.patch >= compatible_version.patch)
                || (compatible_version.major != 0
                    && self.minor > compatible_version.minor))
        {
            Compatibility::Compatible
        } else {
            Compatibility::Incompatible
        }
    }
}

#[must_use]
pub enum Compatibility {
    Compatible,
    Incompatible,
}

type VersionSegment = u16;
