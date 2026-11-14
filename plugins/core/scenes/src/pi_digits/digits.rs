//! Decimal Pi digit generation without network access or floating point.
//!
//! A scene runs on the frame thread of its host and has no worker threads, so the quadratic
//! spigot is a resumable state machine: [`DigitGenerator::step`] does a bounded amount of
//! work per call and the scene shows nothing until the prefix is complete.

pub const MAX_DIGITS: usize = 20_000;

/// Rabinowitz/Wagon base-ten spigot. Integer carry deferral handles runs of nines without
/// any floating-point precision assumption. Extra guard digits resolve the last pending
/// carry; the caller receives exactly `count` digits.
pub struct DigitGenerator {
    count: usize,
    remainders: Vec<u64>,
    output: String,
    pending_digit: u64,
    pending_nines: usize,
    iterations_done: usize,
    iterations_total: usize,
    result: Option<Result<String, String>>,
}

impl DigitGenerator {
    /// A generator for the first `count` digits (3 included, separator excluded).
    pub fn new(count: usize) -> Result<Self, String> {
        if count > MAX_DIGITS {
            return Err(format!("Pi generation is bounded to {MAX_DIGITS} digits"));
        }
        let iterations_total = count + 10;
        let mut generator = Self {
            count,
            remainders: vec![2_u64; iterations_total * 10 / 3 + 1],
            output: String::with_capacity(iterations_total + 2),
            pending_digit: 0,
            pending_nines: 0,
            iterations_done: 0,
            iterations_total,
            result: None,
        };
        if count == 0 {
            generator.result = Some(Ok(String::new()));
        }
        Ok(generator)
    }

    /// Runs spigot iterations until about `budget` inner steps are spent (at least one
    /// iteration); true once the digits are ready or the generation failed.
    pub fn step(&mut self, budget: usize) -> bool {
        if self.result.is_some() {
            return true;
        }
        let per_iteration = self.remainders.len().max(1);
        let iterations = (budget / per_iteration).max(1);
        for _ in 0..iterations {
            self.iterate();
            if self.output.len() > self.count || self.iterations_done >= self.iterations_total {
                self.finish();
                return true;
            }
        }
        false
    }

    /// The digits once ready; `None` while the generation is still running.
    pub fn result(&self) -> Option<&Result<String, String>> {
        self.result.as_ref()
    }

    /// Iterations done out of the total, for a status line.
    pub fn progress(&self) -> (usize, usize) {
        (self.iterations_done, self.iterations_total)
    }

    fn iterate(&mut self) {
        let mut carry = 0_u64;
        for index in (1..=self.remainders.len()).rev() {
            let numerator = 10 * self.remainders[index - 1] + carry * index as u64;
            let denominator = 2 * index as u64 - 1;
            self.remainders[index - 1] = numerator % denominator;
            carry = numerator / denominator;
        }
        self.remainders[0] = carry % 10;
        let digit = carry / 10;
        match digit {
            9 => self.pending_nines += 1,
            10 => {
                self.output
                    .push(char::from(b'0' + (self.pending_digit + 1) as u8));
                self.output
                    .extend(std::iter::repeat_n('0', self.pending_nines));
                self.pending_digit = 0;
                self.pending_nines = 0;
            }
            _ => {
                self.output
                    .push(char::from(b'0' + self.pending_digit as u8));
                self.output
                    .extend(std::iter::repeat_n('9', self.pending_nines));
                self.pending_digit = digit;
                self.pending_nines = 0;
            }
        }
        self.iterations_done += 1;
    }

    fn finish(&mut self) {
        // The first emitted predigit is the sentinel zero, not part of Pi.
        self.result = Some(if self.output.len() <= self.count {
            Err("Pi carry did not resolve within guard digits".into())
        } else {
            Ok(self.output[1..=self.count].to_owned())
        });
        self.remainders = Vec::new();
    }
}

/// The first `count` digits of Pi, computed in one go (the reference of the tests).
#[cfg(test)]
pub fn generate(count: usize) -> Result<String, String> {
    let mut generator = DigitGenerator::new(count)?;
    while !generator.step(usize::MAX / 4) {}
    generator
        .result
        .take()
        .unwrap_or_else(|| Err("Pi generation ended without a result".into()))
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

    #[test]
    fn small_budgets_give_the_same_digits_as_one_pass() {
        let mut generator = DigitGenerator::new(200).unwrap();
        let mut calls = 0;
        while !generator.step(1) {
            calls += 1;
            assert!(calls < 1000, "generation must terminate");
        }
        assert!(calls > 10, "a tiny budget spreads the work over many calls");
        assert_eq!(
            generator.result().unwrap().as_ref().unwrap(),
            &generate(200).unwrap()
        );
        assert_eq!(generator.progress().0, calls + 1);
    }
}
