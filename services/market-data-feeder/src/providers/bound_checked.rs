use std::{
    cmp,
    fmt::{self, Debug, Formatter},
    ops::Sub,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct U8<const MIN: u8, const MAX: u8>(u8);

impl<const MIN: u8, const MAX: u8> U8<MIN, MAX> {
    pub const fn new(value: u8) -> Option<Self> {
        () = const {
            if MAX < MIN {
                unimplemented!()
            }
        };

        if MIN <= value && value <= MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<const MIN: u8, const MAX: u8> Debug for U8<MIN, MAX> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl<const MIN: u8, const MAX: u8> PartialEq<u8> for U8<MIN, MAX> {
    #[inline]
    fn eq(&self, &other: &u8) -> bool {
        self.0 == other
    }
}

impl<const MIN: u8, const MAX: u8> PartialOrd<u8> for U8<MIN, MAX> {
    #[inline]
    fn partial_cmp(&self, other: &u8) -> Option<cmp::Ordering> {
        Some(self.0.cmp(other))
    }
}

impl<const MIN: u8, const MAX: u8> Sub for U8<MIN, MAX> {
    type Output = Option<Self>;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        self - rhs.get()
    }
}

impl<const MIN: u8, const MAX: u8> Sub<u8> for U8<MIN, MAX> {
    type Output = Option<Self>;

    fn sub(self, rhs: u8) -> Self::Output {
        self.get()
            .checked_sub(rhs)
            .and_then(|result| (MIN <= result).then_some(Self(result)))
    }
}

impl<const MIN: u8, const MAX: u8> Sub<U8<MIN, MAX>> for u8 {
    type Output = Option<U8<MIN, MAX>>;

    fn sub(self, rhs: U8<MIN, MAX>) -> Self::Output {
        self.checked_sub(rhs.get()).and_then(U8::new)
    }
}

#[cfg(test)]
impl<const MIN: u8, const MAX: u8> proptest::arbitrary::Arbitrary
    for U8<MIN, MAX>
{
    type Parameters = Option<std::ops::RangeInclusive<u8>>;

    #[inline]
    fn arbitrary_with(range: Self::Parameters) -> Self::Strategy {
        proptest::strategy::statics::Map::new(
            if let Some(range) = range {
                MIN.min(*range.start())..=MAX.max(*range.end())
            } else {
                const { MIN..=MAX }
            },
            Self,
        )
    }

    type Strategy = proptest::strategy::statics::Map<
        std::ops::RangeInclusive<u8>,
        fn(u8) -> Self,
    >;
}
