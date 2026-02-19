//! Runtime value type for the pirc scripting interpreter.

use std::cmp::Ordering;
use std::fmt;

use super::RuntimeError;

/// A runtime value in the scripting language.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A string value.
    String(std::string::String),
    /// A 64-bit signed integer.
    Int(i64),
    /// A 64-bit floating-point number.
    Number(f64),
    /// A boolean value.
    Bool(bool),
    /// The null/empty value.
    Null,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{s}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Null => write!(f, ""),
        }
    }
}

impl Value {
    /// Returns whether this value is "truthy" (evaluates to true in boolean
    /// context).
    ///
    /// - `Null` is falsy
    /// - `Bool(false)` is falsy
    /// - `Int(0)` is falsy
    /// - `Number(0.0)` is falsy
    /// - `String("")` is falsy
    /// - Everything else is truthy
    #[must_use]
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(n) => *n != 0,
            Self::Number(n) => *n != 0.0,
            Self::String(s) => !s.is_empty(),
        }
    }

    /// Equality comparison. Strings are compared case-insensitively following
    /// mIRC conventions. Numeric types are coerced for comparison.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a.eq_ignore_ascii_case(b),
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Int(a), Self::Number(b)) | (Self::Number(b), Self::Int(a)) => {
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                fa == *b
            }
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Null, Self::Null) => true,
            _ => false,
        }
    }

    /// Addition: int+int=int, int+float=float, string+string=concat.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for incompatible types.
    pub fn add(&self, other: &Self) -> Result<Self, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_add(*b))),
            (Self::Number(a), Self::Number(b)) => Ok(Self::Number(a + b)),
            (Self::Int(a), Self::Number(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                Ok(Self::Number(fa + b))
            }
            (Self::Number(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                Ok(Self::Number(a + fb))
            }
            (Self::String(a), Self::String(b)) => {
                let mut result = a.clone();
                result.push_str(b);
                Ok(Self::String(result))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot add {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// Subtraction.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-numeric types.
    pub fn sub(&self, other: &Self) -> Result<Self, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_sub(*b))),
            (Self::Number(a), Self::Number(b)) => Ok(Self::Number(a - b)),
            (Self::Int(a), Self::Number(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                Ok(Self::Number(fa - b))
            }
            (Self::Number(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                Ok(Self::Number(a - fb))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot subtract {} from {}",
                other.type_name(),
                self.type_name()
            ))),
        }
    }

    /// Multiplication.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-numeric types.
    pub fn mul(&self, other: &Self) -> Result<Self, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(Self::Int(a.wrapping_mul(*b))),
            (Self::Number(a), Self::Number(b)) => Ok(Self::Number(a * b)),
            (Self::Int(a), Self::Number(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                Ok(Self::Number(fa * b))
            }
            (Self::Number(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                Ok(Self::Number(a * fb))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot multiply {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// Division.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::DivisionByZero`] for zero divisors, or
    /// [`RuntimeError::TypeError`] for non-numeric types.
    pub fn div(&self, other: &Self) -> Result<Self, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Self::Int(a / b))
            }
            (Self::Number(a), Self::Number(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Self::Number(a / b))
            }
            (Self::Int(a), Self::Number(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                Ok(Self::Number(fa / b))
            }
            (Self::Number(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                Ok(Self::Number(a / fb))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot divide {} by {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// Modulo (remainder).
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::DivisionByZero`] for zero divisors, or
    /// [`RuntimeError::TypeError`] for non-numeric types.
    pub fn modulo(&self, other: &Self) -> Result<Self, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Self::Int(a % b))
            }
            (Self::Number(a), Self::Number(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                Ok(Self::Number(a % b))
            }
            (Self::Int(a), Self::Number(b)) => {
                if *b == 0.0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                Ok(Self::Number(fa % b))
            }
            (Self::Number(a), Self::Int(b)) => {
                if *b == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                Ok(Self::Number(a % fb))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot compute modulo of {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// Numeric negation.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-numeric types.
    pub fn negate(&self) -> Result<Self, RuntimeError> {
        match self {
            Self::Int(n) => Ok(Self::Int(-n)),
            Self::Number(n) => Ok(Self::Number(-n)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot negate {}",
                self.type_name()
            ))),
        }
    }

    /// Ordered comparison. Returns `Bool(true)` if the comparison matches
    /// the given ordering.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-comparable types.
    pub fn compare(&self, other: &Self, expected: Ordering) -> Result<Self, RuntimeError> {
        let ord = self.partial_cmp_values(other)?;
        Ok(Self::Bool(ord == expected))
    }

    /// Less-than-or-equal comparison.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-comparable types.
    pub fn compare_lte(&self, other: &Self) -> Result<Self, RuntimeError> {
        let ord = self.partial_cmp_values(other)?;
        Ok(Self::Bool(ord == Ordering::Less || ord == Ordering::Equal))
    }

    /// Greater-than-or-equal comparison.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::TypeError`] for non-comparable types.
    pub fn compare_gte(&self, other: &Self) -> Result<Self, RuntimeError> {
        let ord = self.partial_cmp_values(other)?;
        Ok(Self::Bool(
            ord == Ordering::Greater || ord == Ordering::Equal,
        ))
    }

    fn partial_cmp_values(&self, other: &Self) -> Result<Ordering, RuntimeError> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Ok(a.cmp(b)),
            (Self::Number(a), Self::Number(b)) => a.partial_cmp(b).ok_or_else(|| {
                RuntimeError::TypeError("cannot compare NaN values".to_string())
            }),
            (Self::Int(a), Self::Number(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fa = *a as f64;
                fa.partial_cmp(b).ok_or_else(|| {
                    RuntimeError::TypeError("cannot compare NaN values".to_string())
                })
            }
            (Self::Number(a), Self::Int(b)) => {
                #[allow(clippy::cast_precision_loss)]
                let fb = *b as f64;
                a.partial_cmp(&fb).ok_or_else(|| {
                    RuntimeError::TypeError("cannot compare NaN values".to_string())
                })
            }
            (Self::String(a), Self::String(b)) => {
                Ok(a.to_lowercase().cmp(&b.to_lowercase()))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot compare {} and {}",
                self.type_name(),
                other.type_name()
            ))),
        }
    }

    /// Returns a human-readable name for the type of this value.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::String(_) => "string",
            Self::Int(_) => "int",
            Self::Number(_) => "number",
            Self::Bool(_) => "bool",
            Self::Null => "null",
        }
    }
}
