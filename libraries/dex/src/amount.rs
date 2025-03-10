use std::{fmt::Debug, marker::PhantomData};

pub trait Marker: Debug + Copy + Eq {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {}

impl Marker for Base {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {}

impl Marker for Quote {}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount<T>
where
    T: Marker,
{
    amount: Decimal,
    _marker: PhantomData<T>,
}

impl<T> Amount<T>
where
    T: Marker,
{
    #[inline]
    pub const fn new(amount: Decimal) -> Self {
        Self {
            amount,
            _marker: const { PhantomData },
        }
    }

    #[inline]
    pub const fn as_inner(&self) -> &Decimal {
        &self.amount
    }

    #[inline]
    pub fn into_inner(self) -> Decimal {
        self.amount
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    amount: String,
    decimal_places: u8,
}

impl Decimal {
    #[inline]
    pub const fn new(amount: String, decimal_places: u8) -> Self {
        Self {
            amount,
            decimal_places,
        }
    }

    #[inline]
    #[must_use]
    pub fn amount(&self) -> &str {
        &self.amount
    }

    #[inline]
    #[must_use]
    pub fn into_amount(self) -> String {
        self.amount
    }

    #[inline]
    #[must_use]
    pub const fn decimal_places(&self) -> u8 {
        self.decimal_places
    }
}
