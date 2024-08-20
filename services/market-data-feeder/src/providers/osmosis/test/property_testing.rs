use std::ops::{RangeInclusive, RangeToInclusive};

use fraction::BigFraction;
use proptest::{
    arbitrary::{any, Arbitrary},
    num::u8,
    proptest,
    strategy::{statics::Map as StaticMap, FilterMap, Flatten, Map, Strategy},
    string::{string_regex, RegexGeneratorStrategy},
    test_runner::TestCaseError,
};

use super::{
    super::{greater_than_max_quote_value, Osmosis},
    calculate_and_assert_against_computed,
};

type BoxedFn<T, R> = Box<dyn Fn(T) -> R>;

#[derive(Debug)]
struct TestCase {
    base_digits: u8,
    quote_digits: u8,
    spot_price: String,
}

impl TestCase {
    fn base_digits_range(quote_digits: u8) -> RangeInclusive<u8> {
        quote_digits.saturating_sub(219_u8)..=quote_digits.saturating_add(36_u8)
    }

    fn postfix_zeroes_range(
        quote_digits: u8,
        base_digits: u8,
    ) -> RangeToInclusive<u8> {
        ..=base_digits
            .min(36_u8.wrapping_add(quote_digits).wrapping_sub(base_digits))
    }

    fn regex_generator(
        quote_digits: u8,
        base_digits: u8,
        postfix_zeroes: u8,
    ) -> RegexGeneratorStrategy<String> {
        let regex_string = format!(
            "0*([1-9][0-9]{{0,{middle_digits}}})?[1-9]0{{0,{postfix_zeroes}}}",
            middle_digits =
                73_u16 + u16::from(quote_digits) - u16::from(base_digits),
        );

        string_regex(&regex_string).unwrap()
    }

    fn generate_test_case(
        quote_digits: u8,
        base_digits: u8,
        spot_price: String,
    ) -> Option<TestCase> {
        let minimal_exponent: u8 = 36_u8
            .min(36_u8.wrapping_add(quote_digits).wrapping_sub(base_digits));

        let trimmed = spot_price.trim_start_matches('0');

        if trimmed
            .len()
            .checked_sub(usize::from(minimal_exponent))
            .and_then(|end| trimmed.get(..end))
            .map_or(false, greater_than_max_quote_value)
        {
            None
        } else {
            Some(Self {
                base_digits,
                quote_digits,
                spot_price,
            })
        }
    }
}

impl Arbitrary for TestCase {
    type Parameters = ();

    fn arbitrary_with((): Self::Parameters) -> Self::Strategy {
        Flatten::new(StaticMap::new(any::<u8>(), |quote_digits: u8| {
            let closure = move |base_digits: u8| {
                let closure = move |postfix_zeroes: u8| {
                    let closure = move |spot_price: String| {
                        Self::generate_test_case(
                            quote_digits,
                            base_digits,
                            spot_price,
                        )
                    };

                    Self::regex_generator(
                        quote_digits,
                        base_digits,
                        postfix_zeroes,
                    )
                    .prop_filter_map(
                        "Generated spot price exceeds u128's bounds.",
                        Box::new(closure) as BoxedFn<_, _>,
                    )
                };

                Self::postfix_zeroes_range(quote_digits, base_digits)
                    .prop_flat_map(Box::new(closure) as BoxedFn<_, _>)
            };

            Self::base_digits_range(quote_digits)
                .prop_flat_map(Box::new(closure) as BoxedFn<_, _>)
        }))
    }

    type Strategy = Flatten<
        StaticMap<
            u8::Any,
            fn(
                u8,
            ) -> Flatten<
                Map<
                    RangeInclusive<u8>,
                    BoxedFn<
                        u8,
                        Flatten<
                            Map<
                                RangeToInclusive<u8>,
                                BoxedFn<
                                    u8,
                                    FilterMap<
                                        RegexGeneratorStrategy<String>,
                                        BoxedFn<String, Option<TestCase>>,
                                    >,
                                >,
                            >,
                        >,
                    >,
                >,
            >,
        >,
    >;
}

proptest! {
    #[test]
    fn test(test_case: TestCase) {
        let TestCase {
            base_digits,
            quote_digits,
            ref spot_price,
        } = test_case;

        let _: BigFraction = Osmosis::normalized_price(
            spot_price,
            base_digits,
            quote_digits,
        )
        .map(|(base, quote)| {
            (base.into_inner(), quote.into_inner())
        })
        .map_err(|error| TestCaseError::fail(error.to_string()))
        .and_then(|(base, quote)| {
            calculate_and_assert_against_computed(
                spot_price,
                base_digits,
                quote_digits,
                &base,
                &quote,
            )
        })?;
    }
}
