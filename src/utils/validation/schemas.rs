use super::errors::DetailedValidationError;
use super::{ValidationContext, ValidationResult, ValidationUtils};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ValidationSchema {
    pub fields: HashMap<String, FieldValidator>,
    pub required_fields: Vec<String>,
    pub optional_fields: Vec<String>,
}

impl ValidationSchema {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
            required_fields: Vec::new(),
            optional_fields: Vec::new(),
        }
    }

    pub fn add_required_field(mut self, name: &str, validator: FieldValidator) -> Self {
        self.fields.insert(name.to_string(), validator);
        self.required_fields.push(name.to_string());
        self
    }

    pub fn add_optional_field(mut self, name: &str, validator: FieldValidator) -> Self {
        self.fields.insert(name.to_string(), validator);
        self.optional_fields.push(name.to_string());
        self
    }

    pub fn validate(&self, value: &Value, ctx: &mut ValidationContext) -> ValidationResult<()> {
        let obj = value.as_object().ok_or_else(|| {
            DetailedValidationError::new(&ctx.get_path(), "Expected object", "INVALID_TYPE")
        })?;

        for field_name in &self.required_fields {
            if !obj.contains_key(field_name) {
                return Err(DetailedValidationError::missing_required_field(field_name).into());
            }
        }

        for (field_name, field_value) in obj {
            if let Some(validator) = self.fields.get(field_name) {
                ctx.enter_field(field_name)?;
                validator.validate(field_value, ctx)?;
                ctx.exit_field();
            } else if !self.allow_unknown_fields() {
                return Err(DetailedValidationError::new(
                    field_name,
                    "Unknown field",
                    "UNKNOWN_FIELD",
                )
                .into());
            }
        }

        Ok(())
    }

    pub fn allow_unknown_fields(&self) -> bool {
        true
    }
}

impl Default for ValidationSchema {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct FieldValidator {
    pub rules: Vec<ValidationRule>,
}

impl FieldValidator {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    pub fn string() -> Self {
        Self::new().add_rule(ValidationRule::Type(FieldType::String))
    }

    pub fn number() -> Self {
        Self::new().add_rule(ValidationRule::Type(FieldType::Number))
    }

    pub fn boolean() -> Self {
        Self::new().add_rule(ValidationRule::Type(FieldType::Boolean))
    }

    pub fn array() -> Self {
        Self::new().add_rule(ValidationRule::Type(FieldType::Array))
    }

    pub fn object() -> Self {
        Self::new().add_rule(ValidationRule::Type(FieldType::Object))
    }

    pub fn min_length(mut self, min: usize) -> Self {
        self.rules.push(ValidationRule::MinLength(min));
        self
    }

    pub fn max_length(mut self, max: usize) -> Self {
        self.rules.push(ValidationRule::MaxLength(max));
        self
    }

    pub fn min_value(mut self, min: f64) -> Self {
        self.rules.push(ValidationRule::MinValue(min));
        self
    }

    pub fn max_value(mut self, max: f64) -> Self {
        self.rules.push(ValidationRule::MaxValue(max));
        self
    }

    pub fn pattern(mut self, regex: &str) -> Self {
        self.rules.push(ValidationRule::Pattern(regex.to_string()));
        self
    }

    pub fn email(mut self) -> Self {
        self.rules.push(ValidationRule::Email);
        self
    }

    pub fn url(mut self) -> Self {
        self.rules.push(ValidationRule::Url);
        self
    }

    pub fn hex_color(mut self) -> Self {
        self.rules.push(ValidationRule::HexColor);
        self
    }

    pub fn uuid(mut self) -> Self {
        self.rules.push(ValidationRule::Uuid);
        self
    }

    pub fn validate(&self, value: &Value, ctx: &mut ValidationContext) -> ValidationResult<()> {
        for rule in &self.rules {
            rule.validate(value, ctx)?;
        }
        Ok(())
    }
}

impl Default for FieldValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum ValidationRule {
    Type(FieldType),
    MinLength(usize),
    MaxLength(usize),
    MinValue(f64),
    MaxValue(f64),
    Pattern(String),
    Email,
    Url,
    HexColor,
    Uuid,
    Custom(fn(&Value, &ValidationContext) -> ValidationResult<()>),
}

impl ValidationRule {
    pub fn validate(&self, value: &Value, ctx: &ValidationContext) -> ValidationResult<()> {
        match self {
            ValidationRule::Type(field_type) => self.validate_type(value, field_type, ctx),
            ValidationRule::MinLength(min) => self.validate_min_length(value, *min, ctx),
            ValidationRule::MaxLength(max) => self.validate_max_length(value, *max, ctx),
            ValidationRule::MinValue(min) => self.validate_min_value(value, *min, ctx),
            ValidationRule::MaxValue(max) => self.validate_max_value(value, *max, ctx),
            ValidationRule::Pattern(pattern) => self.validate_pattern(value, pattern, ctx),
            ValidationRule::Email => self.validate_email(value, ctx),
            ValidationRule::Url => self.validate_url(value, ctx),
            ValidationRule::HexColor => self.validate_hex_color(value, ctx),
            ValidationRule::Uuid => self.validate_uuid(value, ctx),
            ValidationRule::Custom(validator) => validator(value, ctx),
        }
    }

    fn validate_type(
        &self,
        value: &Value,
        expected_type: &FieldType,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let matches = match (expected_type, value) {
            (FieldType::String, Value::String(_)) => true,
            (FieldType::Number, Value::Number(_)) => true,
            (FieldType::Boolean, Value::Bool(_)) => true,
            (FieldType::Array, Value::Array(_)) => true,
            (FieldType::Object, Value::Object(_)) => true,
            (FieldType::Null, Value::Null) => true,
            _ => false,
        };

        if !matches {
            return Err(DetailedValidationError::new(
                &ctx.get_path(),
                &format!(
                    "Expected {}, got {}",
                    expected_type,
                    self.get_value_type(value)
                ),
                "TYPE_MISMATCH",
            )
            .into());
        }

        Ok(())
    }

    fn validate_min_length(
        &self,
        value: &Value,
        min: usize,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let length = match value {
            Value::String(s) => s.len(),
            Value::Array(a) => a.len(),
            _ => {
                return Err(DetailedValidationError::new(
                    &ctx.get_path(),
                    "Length validation only applies to strings and arrays",
                    "INVALID_TYPE",
                )
                .into())
            }
        };

        if length < min {
            return Err(DetailedValidationError::new(
                &ctx.get_path(),
                &format!("Minimum length is {}, got {}", min, length),
                "TOO_SHORT",
            )
            .into());
        }

        Ok(())
    }

    fn validate_max_length(
        &self,
        value: &Value,
        max: usize,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let length = match value {
            Value::String(s) => s.len(),
            Value::Array(a) => a.len(),
            _ => {
                return Err(DetailedValidationError::new(
                    &ctx.get_path(),
                    "Length validation only applies to strings and arrays",
                    "INVALID_TYPE",
                )
                .into())
            }
        };

        if length > max {
            return Err(DetailedValidationError::new(
                &ctx.get_path(),
                &format!("Maximum length is {}, got {}", max, length),
                "TOO_LONG",
            )
            .into());
        }

        Ok(())
    }

    fn validate_min_value(
        &self,
        value: &Value,
        min: f64,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let number = value.as_f64().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Value validation only applies to numbers",
                "INVALID_TYPE",
            )
        })?;

        if number < min {
            return Err(DetailedValidationError::out_of_range(
                &ctx.get_path(),
                number,
                min,
                f64::INFINITY,
            )
            .into());
        }

        Ok(())
    }

    fn validate_max_value(
        &self,
        value: &Value,
        max: f64,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let number = value.as_f64().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Value validation only applies to numbers",
                "INVALID_TYPE",
            )
        })?;

        if number > max {
            return Err(DetailedValidationError::out_of_range(
                &ctx.get_path(),
                number,
                f64::NEG_INFINITY,
                max,
            )
            .into());
        }

        Ok(())
    }

    fn validate_pattern(
        &self,
        value: &Value,
        pattern: &str,
        ctx: &ValidationContext,
    ) -> ValidationResult<()> {
        let string = value.as_str().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Pattern validation only applies to strings",
                "INVALID_TYPE",
            )
        })?;

        let regex = regex::Regex::new(pattern).map_err(|_| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Invalid regex pattern",
                "INVALID_PATTERN",
            )
        })?;

        if !regex.is_match(string) {
            return Err(DetailedValidationError::pattern_mismatch(
                &ctx.get_path(),
                pattern,
                string,
            )
            .into());
        }

        Ok(())
    }

    fn validate_email(&self, value: &Value, ctx: &ValidationContext) -> ValidationResult<()> {
        let string = value.as_str().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Email validation only applies to strings",
                "INVALID_TYPE",
            )
        })?;

        ValidationUtils::validate_email(string, &ctx.get_path())
    }

    fn validate_url(&self, value: &Value, ctx: &ValidationContext) -> ValidationResult<()> {
        let string = value.as_str().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "URL validation only applies to strings",
                "INVALID_TYPE",
            )
        })?;

        ValidationUtils::validate_url(string, &ctx.get_path())
    }

    fn validate_hex_color(&self, value: &Value, ctx: &ValidationContext) -> ValidationResult<()> {
        let string = value.as_str().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "Hex color validation only applies to strings",
                "INVALID_TYPE",
            )
        })?;

        ValidationUtils::validate_hex_color(string, &ctx.get_path())
    }

    fn validate_uuid(&self, value: &Value, ctx: &ValidationContext) -> ValidationResult<()> {
        let string = value.as_str().ok_or_else(|| {
            DetailedValidationError::new(
                &ctx.get_path(),
                "UUID validation only applies to strings",
                "INVALID_TYPE",
            )
        })?;

        ValidationUtils::validate_uuid(string, &ctx.get_path())
    }

    fn get_value_type(&self, value: &Value) -> &'static str {
        match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Array,
    Object,
    Null,
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let type_str = match self {
            FieldType::String => "string",
            FieldType::Number => "number",
            FieldType::Boolean => "boolean",
            FieldType::Array => "array",
            FieldType::Object => "object",
            FieldType::Null => "null",
        };
        write!(f, "{}", type_str)
    }
}

pub struct ApiSchemas;

impl ApiSchemas {
    pub fn settings_update() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field("visualisation", FieldValidator::object())
            .add_optional_field("xr", FieldValidator::object())
            .add_optional_field("system", FieldValidator::object())
            .add_optional_field("rendering", FieldValidator::object())
    }

    pub fn physics_params() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field(
                "damping",
                FieldValidator::number().min_value(0.0).max_value(1.0),
            )
            .add_optional_field(
                "iterations",
                FieldValidator::number().min_value(1.0).max_value(10000.0),
            )
            .add_optional_field(
                "springK",
                FieldValidator::number().min_value(0.0001).max_value(10.0),
            )
            .add_optional_field(
                "repelK",
                FieldValidator::number()
                    .min_value(0.0001)
                    .max_value(10000.0),
            )
            .add_optional_field(
                "attractionK",
                FieldValidator::number().min_value(0.0).max_value(10.0),
            )
            .add_optional_field(
                "boundsSize",
                FieldValidator::number().min_value(1.0).max_value(100000.0),
            )
            .add_optional_field(
                "separationRadius",
                FieldValidator::number().min_value(0.01).max_value(100.0),
            )
            .add_optional_field(
                "maxVelocity",
                FieldValidator::number().min_value(0.001).max_value(1000.0),
            )
            .add_optional_field(
                "massScale",
                FieldValidator::number().min_value(0.01).max_value(100.0),
            )
            .add_optional_field(
                "boundaryDamping",
                FieldValidator::number().min_value(0.0).max_value(1.0),
            )
            .add_optional_field(
                "timeStep",
                FieldValidator::number().min_value(0.001).max_value(1.0),
            )
            .add_optional_field(
                "dt",
                FieldValidator::number().min_value(0.001).max_value(1.0),
            )
            .add_optional_field(
                "temperature",
                FieldValidator::number().min_value(0.0).max_value(100.0),
            )
            .add_optional_field(
                "gravity",
                FieldValidator::number().min_value(-100.0).max_value(100.0),
            )
            .add_optional_field(
                "updateThreshold",
                FieldValidator::number().min_value(0.0).max_value(10.0),
            )
            .add_optional_field("autoBalance", FieldValidator::boolean())
            .add_optional_field(
                "computeMode",
                FieldValidator::number().min_value(0.0).max_value(3.0),
            )
    }

    pub fn ragflow_chat() -> ValidationSchema {
        ValidationSchema::new()
            .add_required_field(
                "question",
                FieldValidator::string().min_length(1).max_length(10000),
            )
            .add_optional_field("session_id", FieldValidator::string().max_length(255))
            .add_optional_field("stream", FieldValidator::boolean())
            .add_optional_field("enable_tts", FieldValidator::boolean())
    }

    pub fn bots_data() -> ValidationSchema {
        ValidationSchema::new()
            .add_required_field("nodes", FieldValidator::array().max_length(1000))
            .add_required_field("edges", FieldValidator::array().max_length(10000))
    }

    pub fn swarm_init() -> ValidationSchema {
        ValidationSchema::new()
            .add_required_field(
                "topology",
                FieldValidator::string().pattern("^(mesh|hierarchical|ring|star)$"),
            )
            .add_required_field(
                "max_agents",
                FieldValidator::number().min_value(1.0).max_value(100.0),
            )
            .add_required_field("strategy", FieldValidator::string().max_length(50))
            .add_optional_field("enable_neural", FieldValidator::boolean())
            .add_optional_field("agent_types", FieldValidator::array())
            .add_optional_field("custom_prompt", FieldValidator::string().max_length(5000))
    }

    pub fn node_settings() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field("baseColor", FieldValidator::string().hex_color())
            .add_optional_field(
                "opacity",
                FieldValidator::number().min_value(0.0).max_value(1.0),
            )
            .add_optional_field(
                "metalness",
                FieldValidator::number().min_value(0.0).max_value(1.0),
            )
            .add_optional_field(
                "roughness",
                FieldValidator::number().min_value(0.0).max_value(1.0),
            )
            .add_optional_field(
                "nodeSize",
                FieldValidator::number().min_value(0.1).max_value(1000.0),
            )
            .add_optional_field(
                "quality",
                FieldValidator::string().pattern("^(low|medium|high)$"),
            )
    }

    pub fn xr_settings() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field("enabled", FieldValidator::boolean())
            .add_optional_field(
                "quality",
                FieldValidator::string().pattern("^(low|medium|high)$"),
            )
            .add_optional_field(
                "renderScale",
                FieldValidator::number().min_value(0.1).max_value(10.0),
            )
            .add_optional_field(
                "roomScale",
                FieldValidator::number().min_value(0.1).max_value(100.0),
            )
            .add_optional_field("handTracking", FieldValidator::object())
            .add_optional_field("interactions", FieldValidator::object())
    }

    pub fn rendering_settings() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field(
                "ambientLightIntensity",
                FieldValidator::number().min_value(0.0).max_value(100.0),
            )
            .add_optional_field("bloom", Self::bloom_glow_effects())
            .add_optional_field("glow", Self::bloom_glow_effects())
    }

    fn bloom_glow_effects() -> FieldValidator {
        FieldValidator::object()
    }

    pub fn complete_rendering_schema() -> ValidationSchema {
        ValidationSchema::new()
            .add_optional_field(
                "ambientLightIntensity",
                FieldValidator::number().min_value(0.0).max_value(100.0),
            )
            .add_optional_field("bloom", FieldValidator::object())
            .add_optional_field("glow", FieldValidator::object())
            .add_optional_field("postProcessing", FieldValidator::object())
            .add_optional_field("effects", FieldValidator::object())
    }
}
