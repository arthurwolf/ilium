//! Decimal Pi digit generation without network access or floating point.
pub const MAX_DIGITS: usize = 20_000;

/// Rabinowitz/Wagon base-ten spigot. Returns digits beginning with `3`,
/// excluding the decimal separator. Integer carry deferral handles runs of
/// nines without any floating-point precision assumption.
///
/// Quadratic work is deliberately bounded and belongs on an owned worker,
/// never on the presentation thread. Extra guard digits resolve the last
/// pending carry; the caller receives exactly `count` digits.
pub fn generate(count: usize) -> Result<String, String> {
    if count > MAX_DIGITS {
        return Err(format!("Pi generation is bounded to {MAX_DIGITS} digits"));
    }
    if count == 0 {
        return Ok(String::new());
    }
    let iterations = count + 10;
    let mut remainders = vec![2_u64; iterations * 10 / 3 + 1];
    let mut output = String::with_capacity(iterations + 2);
    let mut pending_digit = 0_u64;
    let mut pending_nines = 0;
    for _ in 0..iterations {
        let mut carry = 0_u64;
        for index in (1..=remainders.len()).rev() {
            let numerator = 10 * remainders[index - 1] + carry * index as u64;
            let denominator = 2 * index as u64 - 1;
            remainders[index - 1] = numerator % denominator;
            carry = numerator / denominator;
        }
        remainders[0] = carry % 10;
        let digit = carry / 10;
        match digit {
            9 => pending_nines += 1,
            10 => {
                output.push(char::from(b'0' + (pending_digit + 1) as u8));
                output.extend(std::iter::repeat_n('0', pending_nines));
                pending_digit = 0;
                pending_nines = 0;
            }
            _ => {
                output.push(char::from(b'0' + pending_digit as u8));
                output.extend(std::iter::repeat_n('9', pending_nines));
                pending_digit = digit;
                pending_nines = 0;
            }
        }
        if output.len() > count {
            break;
        }
    }
    // The first emitted predigit is the sentinel zero, not part of Pi.
    if output.len() <= count {
        return Err("Pi carry did not resolve within guard digits".into());
    }
    Ok(output[1..=count].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_prefix_matches_independent_decimal_reference() {
        let expected = "31415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679";
        assert_eq!(generate(expected.len()).unwrap(), expected);
    }
    #[test]
    fn prefix_stays_exact_when_requested_length_changes() {
        let long = generate(1000).unwrap();
        for length in [1, 2, 3, 9, 16, 31, 99, 250] {
            assert_eq!(generate(length).unwrap(), &long[..length]);
        }
        assert!(generate(0).unwrap().is_empty());
        assert!(generate(MAX_DIGITS + 1).is_err());
    }
}
