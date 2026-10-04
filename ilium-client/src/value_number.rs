//! Typed validation for direct keyboard entry in numeric UI controls.
//! Callers keep mutation, units, persistence and domain-specific validation.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumberValue {
    Integer(i128),
    Decimal(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumberSpec {
    Integer { minimum: i128, maximum: i128 },
    Decimal { minimum: f64, maximum: f64 },
}

impl NumberSpec {
    /// Step toward the requested bound; numeric controls never wrap.
    /// The caller supplies its domain step rather than quantizing typed values.
    pub fn stepped(
        self,
        value: NumberValue,
        step: NumberValue,
        direction: i32,
    ) -> Result<NumberValue, String> {
        self.validate(value)?;
        match (self, value, step) {
            (
                Self::Integer { minimum, maximum },
                NumberValue::Integer(value),
                NumberValue::Integer(step),
            ) => {
                if step <= 0 {
                    return Err("Numeric step must be positive".to_string());
                }
                let next = match direction.signum() {
                    -1 => value.checked_sub(step).unwrap_or(minimum),
                    1 => value.checked_add(step).unwrap_or(maximum),
                    _ => value,
                };
                Ok(NumberValue::Integer(next.clamp(minimum, maximum)))
            }
            (
                Self::Decimal { minimum, maximum },
                NumberValue::Decimal(value),
                NumberValue::Decimal(step),
            ) => {
                if !step.is_finite() || step <= 0.0 {
                    return Err("Numeric step must be finite and positive".to_string());
                }
                let next = value + step * f64::from(direction.signum());
                // An overflowing step reaches the finite domain bound.
                Ok(NumberValue::Decimal(next.clamp(minimum, maximum)))
            }
            _ => Err("Numeric value and step must match the control's type".to_string()),
        }
    }

    /// Check a value against the supplied domain bounds without changing it.
    pub fn validate(self, value: NumberValue) -> Result<(), String> {
        match (self, value) {
            (Self::Integer { minimum, maximum }, NumberValue::Integer(value)) => {
                if minimum > maximum {
                    return Err("Invalid numeric control bounds".to_string());
                }
                if !(minimum..=maximum).contains(&value) {
                    return Err(format!("Enter a number between {minimum} and {maximum}"));
                }
            }
            (Self::Decimal { minimum, maximum }, NumberValue::Decimal(value)) => {
                if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
                    return Err("Invalid numeric control bounds".to_string());
                }
                if !value.is_finite() {
                    return Err("Enter a finite number".to_string());
                }
                if !(minimum..=maximum).contains(&value) {
                    return Err(format!("Enter a number between {minimum} and {maximum}"));
                }
            }
            _ => return Err("Numeric value must match the control's type".to_string()),
        }
        Ok(())
    }

    /// Parse an exact entry without applying a slider's step quantization.
    /// Bounds are validated before comparison, including non-finite floats.
    pub fn parse(self, text: &str) -> Result<NumberValue, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Enter a number".to_string());
        }
        let value = match self {
            Self::Integer { .. } => NumberValue::Integer(
                text.parse::<i128>()
                    .map_err(|_| "Enter a whole number within the supported range".to_string())?,
            ),
            Self::Decimal { .. } => NumberValue::Decimal(
                text.parse::<f64>()
                    .map_err(|_| "Enter a valid number".to_string())?,
            ),
        };
        self.validate(value)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_entry_accepts_exact_values_without_slider_quantization() {
        let spec = NumberSpec::Integer {
            minimum: -100,
            maximum: 100,
        };
        assert_eq!(spec.parse(" 17 "), Ok(NumberValue::Integer(17)));
        assert_eq!(spec.parse("-100"), Ok(NumberValue::Integer(-100)));
        assert_eq!(spec.parse("+100"), Ok(NumberValue::Integer(100)));
    }

    #[test]
    fn integer_entry_does_not_round_clamp_or_overflow() {
        let spec = NumberSpec::Integer {
            minimum: 0,
            maximum: 100,
        };
        for invalid in [
            "",
            "   ",
            "17.5",
            "17 MiB",
            "-1",
            "101",
            "170141183460469231731687303715884105728",
        ] {
            assert!(spec.parse(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(
            NumberSpec::Integer {
                minimum: 0,
                maximum: i128::MAX
            }
            .parse(&i128::MAX.to_string()),
            Ok(NumberValue::Integer(i128::MAX))
        );
    }

    #[test]
    fn decimal_entry_accepts_fractional_and_scientific_values() {
        let spec = NumberSpec::Decimal {
            minimum: 0.0,
            maximum: 100.0,
        };
        assert_eq!(spec.parse("0.125"), Ok(NumberValue::Decimal(0.125)));
        assert_eq!(spec.parse("1e2"), Ok(NumberValue::Decimal(100.0)));
        assert_eq!(spec.parse(" 0 "), Ok(NumberValue::Decimal(0.0)));
    }

    #[test]
    fn decimal_entry_rejects_nonfinite_and_out_of_range_values() {
        let spec = NumberSpec::Decimal {
            minimum: 0.0,
            maximum: 100.0,
        };
        for invalid in [
            "", "NaN", "inf", "-inf", "1e999", "-0.1", "100.001", "1,5", "2 s",
        ] {
            assert!(spec.parse(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn invalid_bounds_are_errors_instead_of_panics_or_accepted_input() {
        assert!(NumberSpec::Integer {
            minimum: 10,
            maximum: 0
        }
        .parse("5")
        .is_err());
        for (minimum, maximum) in [(10.0, 0.0), (f64::NAN, 10.0), (0.0, f64::INFINITY)] {
            assert!(NumberSpec::Decimal { minimum, maximum }.parse("5").is_err());
        }
    }
    #[test]
    fn integer_buttons_move_monotonically_and_stop_at_bounds() {
        let spec = NumberSpec::Integer {
            minimum: 0,
            maximum: 100,
        };
        assert_eq!(
            spec.stepped(NumberValue::Integer(17), NumberValue::Integer(4), 1),
            Ok(NumberValue::Integer(21))
        );
        assert_eq!(
            spec.stepped(NumberValue::Integer(17), NumberValue::Integer(4), -1),
            Ok(NumberValue::Integer(13))
        );
        assert_eq!(
            spec.stepped(NumberValue::Integer(0), NumberValue::Integer(4), -1),
            Ok(NumberValue::Integer(0))
        );
        assert_eq!(
            spec.stepped(NumberValue::Integer(100), NumberValue::Integer(4), 1),
            Ok(NumberValue::Integer(100))
        );
        assert_eq!(
            spec.stepped(NumberValue::Integer(17), NumberValue::Integer(4), 0),
            Ok(NumberValue::Integer(17))
        );
    }

    #[test]
    fn integer_buttons_do_not_overflow_at_storage_limits() {
        let spec = NumberSpec::Integer {
            minimum: i128::MIN,
            maximum: i128::MAX,
        };
        assert_eq!(
            spec.stepped(
                NumberValue::Integer(i128::MAX - 1),
                NumberValue::Integer(4),
                1
            ),
            Ok(NumberValue::Integer(i128::MAX))
        );
        assert_eq!(
            spec.stepped(
                NumberValue::Integer(i128::MIN + 1),
                NumberValue::Integer(4),
                -1
            ),
            Ok(NumberValue::Integer(i128::MIN))
        );
    }

    #[test]
    fn decimal_buttons_preserve_fractional_steps_and_clamp() {
        let spec = NumberSpec::Decimal {
            minimum: 0.0,
            maximum: 1.0,
        };
        assert_eq!(
            spec.stepped(NumberValue::Decimal(0.25), NumberValue::Decimal(0.125), 1),
            Ok(NumberValue::Decimal(0.375))
        );
        assert_eq!(
            spec.stepped(NumberValue::Decimal(0.25), NumberValue::Decimal(0.125), -1),
            Ok(NumberValue::Decimal(0.125))
        );
        assert_eq!(
            spec.stepped(NumberValue::Decimal(0.95), NumberValue::Decimal(0.125), 1),
            Ok(NumberValue::Decimal(1.0))
        );
        assert_eq!(
            NumberSpec::Decimal {
                minimum: 0.0,
                maximum: f64::MAX
            }
            .stepped(
                NumberValue::Decimal(f64::MAX),
                NumberValue::Decimal(f64::MAX),
                1
            ),
            Ok(NumberValue::Decimal(f64::MAX))
        );
    }

    #[test]
    fn buttons_reject_invalid_contracts_and_values() {
        let spec = NumberSpec::Integer {
            minimum: 0,
            maximum: 100,
        };
        assert!(spec
            .stepped(NumberValue::Integer(101), NumberValue::Integer(1), 1)
            .is_err());
        assert!(spec
            .stepped(NumberValue::Integer(10), NumberValue::Integer(0), 1)
            .is_err());
        assert!(spec
            .stepped(NumberValue::Integer(10), NumberValue::Integer(-1), 1)
            .is_err());
        assert!(spec
            .stepped(NumberValue::Integer(10), NumberValue::Decimal(1.0), 1)
            .is_err());
        let decimal = NumberSpec::Decimal {
            minimum: 0.0,
            maximum: 100.0,
        };
        assert!(decimal
            .stepped(NumberValue::Decimal(f64::NAN), NumberValue::Decimal(1.0), 1)
            .is_err());
        assert!(decimal
            .stepped(
                NumberValue::Decimal(10.0),
                NumberValue::Decimal(f64::INFINITY),
                1
            )
            .is_err());
        assert!(NumberSpec::Integer {
            minimum: 10,
            maximum: 0
        }
        .stepped(NumberValue::Integer(5), NumberValue::Integer(1), 1)
        .is_err());
    }
}
