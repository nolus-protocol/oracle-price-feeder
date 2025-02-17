use fraction::{BigFraction, BigUint};
use proptest::{
    prop_assert, prop_assert_eq, prop_assert_ne, test_runner::TestCaseError,
};

use crate::provider::Decimal;

use super::{greater_than_max_quote_value, Osmosis, MAX_QUOTE_VALUE};

mod property_testing;

#[track_caller]
fn calculate_and_assert_against_manual(
    spot_price: &str,
    base_decimal_digits: u8,
    quote_decimal_digits: u8,
    base_digits: u8,
    quote_amount: &str,
    quote_digits: u8,
    decimal_form: &str,
) {
    let (base, quote) = {
        let (base, quote) = Osmosis::normalized_price(
            spot_price,
            base_decimal_digits,
            quote_decimal_digits,
        )
        .unwrap();

        (base.into_inner(), quote.into_inner())
    };

    assert!(!base.amount().is_empty(), "{base:?}");
    assert_eq!(&base.amount()[..1], "1", "{base:?}");
    assert_eq!(
        base.amount()[1..].len(),
        usize::from(base.decimal_places()),
        "{base:?}"
    );
    assert_eq!(base.amount()[1..].trim_end_matches('0'), "", "{base:?}");
    assert_eq!(base.decimal_places(), base_digits, "{base:?}");
    assert!(!greater_than_max_quote_value(base.amount()), "{base:?}");

    assert!(!quote.amount().is_empty(), "{quote:?}");
    assert_eq!(quote.amount(), quote_amount, "{quote:?}");
    assert_eq!(quote.decimal_places(), quote_digits, "{quote:?}");
    assert!(!greater_than_max_quote_value(quote.amount()), "{quote:?}");

    assert_eq!(
        calculate_and_assert_against_computed(
            spot_price,
            base_decimal_digits,
            quote_decimal_digits,
            &base,
            &quote,
        )
        .unwrap(),
        decimal_form.parse().unwrap(),
    );
}

fn calculate_and_assert_against_computed(
    mut spot_price: &str,
    base_digits: u8,
    quote_digits: u8,
    base: &Decimal,
    quote: &Decimal,
) -> Result<BigFraction, TestCaseError> {
    spot_price = spot_price.trim_start_matches('0');

    prop_assert_ne!(spot_price, "");

    prop_assert_eq!(
        base_digits.abs_diff(base.decimal_places()),
        quote_digits.abs_diff(quote.decimal_places()),
        "{:?} / {:?}",
        base,
        quote,
    );

    prop_assert!(base.amount().len() <= 39, "{:?} / {:?}", base, quote,);

    prop_assert!(quote.amount().len() <= 39, "{:?} / {:?}", base, quote,);

    let mut quote_exponent = u8::try_from(
        (36_u16 + u16::from(quote_digits))
            .checked_sub(base_digits.into())
            .unwrap(),
    )?;

    spot_price = {
        let base_exponent;

        (spot_price, base_exponent) = {
            let mut base_exponent: u8 = 36;

            let input = spot_price.trim_end_matches(|ch| {
                if ch == '0' && base_exponent != 0 && quote_exponent != 0 {
                    base_exponent -= 1;

                    quote_exponent -= 1;

                    true
                } else {
                    false
                }
            });

            (input, base_exponent)
        };

        if greater_than_max_quote_value(spot_price) {
            let length =
                if spot_price[..MAX_QUOTE_VALUE.len()] < *MAX_QUOTE_VALUE {
                    MAX_QUOTE_VALUE.len()
                } else {
                    MAX_QUOTE_VALUE.len() - 1
                };

            let excess = u8::try_from(spot_price.len() - length)?;

            prop_assert!(excess <= base_exponent);

            prop_assert!(excess <= quote_exponent);

            spot_price = &spot_price[..length];

            quote_exponent -= excess;
        }

        spot_price
    };

    let actual = BigFraction::new(
        quote.amount().parse::<BigUint>()?
            * BigUint::from(10_u8).pow(base.decimal_places().into()),
        base.amount().parse::<BigUint>()?
            * BigUint::from(10_u8).pow(quote.decimal_places().into()),
    );

    let expected = BigFraction::new(
        spot_price.parse::<BigUint>()?,
        BigUint::from(10_u8).pow(quote_exponent.into()),
    );

    prop_assert_eq!(&actual, &{ expected }, "{:?} / {:?}", quote, base);

    Ok(actual)
}

#[test]
fn equal_digits_real_data() {
    // 12521940695999309011000000000000000000 = 12.521940695999309011
    calculate_and_assert_against_manual(
        "12521940695999309011000000000000000000",
        6,
        6,
        18,
        "12521940695999309011",
        18,
        "12.521940695999309011",
    );
}

#[test]
fn equal_digits() {
    calculate_and_assert_against_manual(
        "1941974700000000000000000000000000",
        6,
        6,
        10,
        "19419747",
        10,
        "0.0019419747",
    );

    calculate_and_assert_against_manual(
        "001941974700000000000000000000000000",
        6,
        6,
        10,
        "19419747",
        10,
        "0.0019419747",
    );

    calculate_and_assert_against_manual(
        "194197470000000000000000000000000000",
        6,
        6,
        8,
        "19419747",
        8,
        "0.19419747",
    );

    calculate_and_assert_against_manual(
        "24602951060000000000000000000000000000",
        6,
        6,
        8,
        "2460295106",
        8,
        "24.60295106",
    );

    calculate_and_assert_against_manual(
        "74167359355013376000000000000000000",
        6,
        6,
        18,
        "74167359355013376",
        18,
        "0.074167359355013376",
    );
}

#[test]
fn more_to_less_digits_real_data() {
    // 1673949300000000000000000000 = 0.0016739493
    calculate_and_assert_against_manual(
        "1673949300000000000000000000",
        12,
        6,
        16,
        "16739493",
        10,
        "0.0016739493",
    );
}

#[test]
fn more_to_less_digits() {
    calculate_and_assert_against_manual("1", 36, 0, 36, "1", 0, "1");

    calculate_and_assert_against_manual("10", 36, 0, 36, "10", 0, "10");

    calculate_and_assert_against_manual(
        "1000000000000000000000000000000000000",
        36,
        0,
        36,
        "1000000000000000000000000000000000000",
        0,
        "1000000000000000000000000000000000000.0",
    );

    calculate_and_assert_against_manual(
        "100000000000000000000000000000000000000",
        36,
        0,
        36,
        "100000000000000000000000000000000000000",
        0,
        "100000000000000000000000000000000000000.0",
    );

    {
        let u128_max = &u128::MAX.to_string();

        calculate_and_assert_against_manual(
            u128_max, 36, 0, 36, u128_max, 0, u128_max,
        );
    }

    calculate_and_assert_against_manual(
        "6724762410000000000000000000",
        18,
        6,
        17,
        "672476241",
        5,
        "6724.76241",
    );

    calculate_and_assert_against_manual(
        "6724762415000000000000000000",
        18,
        6,
        18,
        "6724762415",
        6,
        "6724.762415",
    );

    calculate_and_assert_against_manual(
        "6724762415300000000000000000",
        18,
        6,
        19,
        "67247624153",
        7,
        "6724.7624153",
    );
}

#[test]
fn less_to_more_digits_real_data() {
    // 146191139329904908308095408000000000000000000 = 0.000146191139329904908308095408
    const SPOT_PRICE: &str = "146191139329904908308095408000000000000000000";

    const BASE_DIGITS: u8 = 6;

    const QUOTE_DIGITS: u8 = 18;

    calculate_and_assert_against_manual(
        SPOT_PRICE,
        BASE_DIGITS,
        QUOTE_DIGITS,
        18,
        "146191139329904908308095408",
        30,
        "0.000146191139329904908308095408",
    );
}

#[test]
fn less_to_more_digits() {
    calculate_and_assert_against_manual(
        "2000000000000000000000000000000000002",
        0,
        1,
        36,
        "2000000000000000000000000000000000002",
        37,
        "0.2000000000000000000000000000000000002",
    );

    calculate_and_assert_against_manual(
        "2000000000000000000000000000000000000",
        6,
        7,
        0,
        "2",
        1,
        "0.2",
    );

    calculate_and_assert_against_manual(
        "20000000000000000000000000000000000000",
        6,
        7,
        0,
        "20",
        1,
        "2.0",
    );

    calculate_and_assert_against_manual(
        "200000000000000000000000000000000000000",
        6,
        7,
        0,
        "200",
        1,
        "20.0",
    );

    calculate_and_assert_against_manual(
        "280135606978566858130324052000000000000000000000",
        6,
        18,
        15,
        "280135606978566858130324052",
        27,
        "0.280135606978566858130324052",
    );

    calculate_and_assert_against_manual(
        "280135606978566858130324052500000000000000000000",
        6,
        18,
        16,
        "2801356069785668581303240525",
        28,
        "0.2801356069785668581303240525",
    );

    calculate_and_assert_against_manual(
        "280135606978566858130324052510000000000000000000",
        6,
        18,
        17,
        "28013560697856685813032405251",
        29,
        "0.28013560697856685813032405251",
    );
}
