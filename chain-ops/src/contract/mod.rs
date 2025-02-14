use std::{borrow::Cow, str::FromStr};

use anyhow::Context as _;
use serde::Deserialize;

pub use self::admin::Admin;

pub mod admin;

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Deserialize)]
#[serde(try_from = "Cow<'de, str>")]
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

impl TryFrom<Cow<'_, str>> for SemVer {
    type Error = anyhow::Error;

    #[inline]
    fn try_from(value: Cow<'_, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[must_use]
pub enum Compatibility {
    Compatible,
    Incompatible,
}

type VersionSegment = u16;
