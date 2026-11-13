//! Typed validation for direct keyboard entry in numeric controls (from `ilium-client/src/value_number.rs`).
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
