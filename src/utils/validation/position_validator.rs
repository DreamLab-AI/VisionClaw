use super::errors::DetailedValidationError;
use super::ValidationResult;
use serde_json::Value;

pub struct PositionValidator;

impl PositionValidator {
    pub fn validate_position_value(value: &Value, field: &str) -> ValidationResult<f32> {
        match value {
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if f.is_nan() || f.is_infinite() {
                        return Err(DetailedValidationError::new(
                            field,
                            "Position value cannot be NaN or Infinity",
                            "INVALID_POSITION",
                        )
                        .into());
                    }

                    if f.abs() > 1_000_000.0 {
                        return Err(DetailedValidationError::new(
                            field,
                            "Position value exceeds reasonable bounds",
                            "POSITION_OUT_OF_BOUNDS",
                        )
                        .into());
                    }
                    Ok(f as f32)
                } else {
                    Err(DetailedValidationError::new(
                        field,
                        "Invalid numeric value",
                        "INVALID_NUMBER",
                    )
                    .into())
                }
            }
            Value::String(s) => match s.parse::<f64>() {
                Ok(f) => {
                    if f.is_nan() || f.is_infinite() {
                        return Err(DetailedValidationError::new(
                            field,
                            "Position value cannot be NaN or Infinity",
                            "INVALID_POSITION",
                        )
                        .into());
                    }
                    if f.abs() > 1_000_000.0 {
                        return Err(DetailedValidationError::new(
                            field,
                            "Position value exceeds reasonable bounds",
                            "POSITION_OUT_OF_BOUNDS",
                        )
                        .into());
                    }
                    Ok(f as f32)
                }
                Err(_) => Err(DetailedValidationError::new(
                    field,
                    "Invalid numeric string",
                    "INVALID_NUMBER_FORMAT",
                )
                .into()),
            },
            _ => Err(DetailedValidationError::new(
                field,
                "Position must be a number or numeric string",
                "INVALID_TYPE",
            )
            .into()),
        }
    }

    pub fn validate_position_object(position: &Value) -> ValidationResult<(f32, f32, f32)> {
        let obj = position.as_object().ok_or_else(|| {
            DetailedValidationError::new("position", "Position must be an object", "INVALID_TYPE")
        })?;

        let x = obj
            .get("x")
            .ok_or_else(|| {
                Box::new(DetailedValidationError::new(
                    "position.x",
                    "Missing x coordinate",
                    "MISSING_FIELD",
                ))
            })
            .and_then(|v| Self::validate_position_value(v, "position.x"))?;

        let y = obj
            .get("y")
            .ok_or_else(|| {
                Box::new(DetailedValidationError::new(
                    "position.y",
                    "Missing y coordinate",
                    "MISSING_FIELD",
                ))
            })
            .and_then(|v| Self::validate_position_value(v, "position.y"))?;

        let z = obj
            .get("z")
            .ok_or_else(|| {
                Box::new(DetailedValidationError::new(
                    "position.z",
                    "Missing z coordinate",
                    "MISSING_FIELD",
                ))
            })
            .and_then(|v| Self::validate_position_value(v, "position.z"))?;

        Ok((x, y, z))
    }

    pub fn validate_velocity_value(value: &Value, field: &str) -> ValidationResult<f32> {
        match value {
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    if f.is_nan() || f.is_infinite() {
                        return Err(DetailedValidationError::new(
                            field,
                            "Velocity value cannot be NaN or Infinity",
                            "INVALID_VELOCITY",
                        )
                        .into());
                    }

                    if f.abs() > 10_000.0 {
                        return Err(DetailedValidationError::new(
                            field,
                            "Velocity value exceeds reasonable bounds",
                            "VELOCITY_OUT_OF_BOUNDS",
                        )
                        .into());
                    }
                    Ok(f as f32)
                } else {
                    Err(DetailedValidationError::new(
                        field,
                        "Invalid numeric value",
                        "INVALID_NUMBER",
                    )
                    .into())
                }
            }
            Value::String(s) => match s.parse::<f64>() {
                Ok(f) => {
                    if f.is_nan() || f.is_infinite() {
                        return Err(DetailedValidationError::new(
                            field,
                            "Velocity value cannot be NaN or Infinity",
                            "INVALID_VELOCITY",
                        )
                        .into());
                    }
                    if f.abs() > 10_000.0 {
                        return Err(DetailedValidationError::new(
                            field,
                            "Velocity value exceeds reasonable bounds",
                            "VELOCITY_OUT_OF_BOUNDS",
                        )
                        .into());
                    }
                    Ok(f as f32)
                }
                Err(_) => Err(DetailedValidationError::new(
                    field,
                    "Invalid numeric string",
                    "INVALID_NUMBER_FORMAT",
                )
                .into()),
            },
            _ => Err(DetailedValidationError::new(
                field,
                "Velocity must be a number or numeric string",
                "INVALID_TYPE",
            )
            .into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_position_numeric_string() {
        let value = json!("123.45");
        let result = PositionValidator::validate_position_value(&value, "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 123.45f32);
    }

    #[test]
    fn test_validate_position_number() {
        let value = json!(456.78);
        let result = PositionValidator::validate_position_value(&value, "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 456.78f32);
    }

    #[test]
    fn test_validate_position_invalid() {
        let value = json!("not a number");
        let result = PositionValidator::validate_position_value(&value, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_position_object() {
        let position = json!({
            "x": "100.5",
            "y": 200.0,
            "z": "300.75"
        });
        let result = PositionValidator::validate_position_object(&position);
        assert!(result.is_ok());
        let (x, y, z) = result.unwrap();
        assert_eq!(x, 100.5f32);
        assert_eq!(y, 200.0f32);
        assert_eq!(z, 300.75f32);
    }
}
