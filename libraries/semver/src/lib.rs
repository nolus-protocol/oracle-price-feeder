use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use anyhow::Context as _;
use serde::Deserialize;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
#[must_use]
pub struct SemVer {
    major: VersionSegment,
    minor: VersionSegment,
    patch: VersionSegment,
}

impl Display for SemVer {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "{}.{}.{}",
            self.major, self.minor, self.patch
        ))
    }
}

impl SemVer {
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

impl FromStr for SemVer {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.split_once('.')
            .context("Version doesn't include a major version separator!")
            .and_then(|(major, rest)| {
                rest.split_once('.')
                    .context(
                        "Version doesn't include a major version separator!",
                    )
                    .and_then(|(minor, patch)| {
                        Ok(Self {
                            major: major.parse()?,
                            minor: minor.parse()?,
                            patch: patch.parse()?,
                        })
                    })
            })
    }
}

#[must_use]
pub enum Compatibility {
    Compatible,
    Incompatible,
}

type VersionSegment = u16;

#[test]
fn test_parsing() {
    for invalid_version in [
        "", ".", ".0", "0", "0.", ".0.0", "0.0", "0.0.", ".0.0.0", "0.0.0.",
        ".0.0.0.0", "0.0.0.0", "0.0.0.0.",
    ] {
        invalid_version.parse::<SemVer>().unwrap_err();
    }

    assert_eq!("0.0.0".parse::<SemVer>().unwrap(), SemVer::new(0, 0, 0));

    assert_eq!("1.2.3".parse::<SemVer>().unwrap(), SemVer::new(1, 2, 3));

    assert_eq!(
        "10.02.030".parse::<SemVer>().unwrap(),
        SemVer::new(10, 2, 30)
    );
}
