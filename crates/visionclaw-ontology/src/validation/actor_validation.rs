//! Actor Address Validation
//!
//! Validates that optional actor addresses in AppState are properly initialized
//! based on feature flags and configuration. Helps catch missing dependencies
//! at startup instead of runtime.

use log::{error, info, warn};

/// Validation result for a single optional actor/service
#[derive(Debug, Clone)]
pub struct ValidationItem {
    pub name: String,
    pub expected: bool,
    pub present: bool,
    pub severity: Severity,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    /// Must be present if expected, otherwise critical error
    Critical,
    /// Should be present if expected, but can continue without
    Warning,
    /// Optional, no warning if missing
    Info,
}

/// Complete validation report
#[derive(Debug)]
pub struct ValidationReport {
    pub items: Vec<ValidationItem>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub infos: Vec<String>,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            infos: Vec::new(),
        }
    }

    /// Add a validation item
    pub fn add(&mut self, item: ValidationItem) {
        // Generate messages based on validation result
        if item.expected && !item.present {
            let msg = format!(
                "{} is not initialized but was expected ({})",
                item.name, item.reason
            );
            match item.severity {
                Severity::Critical => self.errors.push(msg.clone()),
                Severity::Warning => self.warnings.push(msg.clone()),
                Severity::Info => self.infos.push(msg.clone()),
            }
        } else if !item.expected && item.present {
            let msg = format!("{} is initialized but not expected", item.name);
            self.infos.push(msg);
        }

        self.items.push(item);
    }

    /// Check if validation passed (no critical errors)
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Log the validation report
    pub fn log(&self) {
        info!("=== AppState Validation Report ===");

        // Log successful validations
        let successful: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.expected == item.present && item.present)
            .collect();

        if !successful.is_empty() {
            info!("Validated {} components:", successful.len());
            for item in successful {
                info!("  {}: {}", item.name, item.reason);
            }
        }

        // Log errors
        if !self.errors.is_empty() {
            error!("❌ {} critical validation errors:", self.errors.len());
            for err in &self.errors {
                error!("  ✗ {}", err);
            }
        }

        // Log warnings
        if !self.warnings.is_empty() {
            warn!("⚠️  {} warnings:", self.warnings.len());
            for warning in &self.warnings {
                warn!("  ⚠ {}", warning);
            }
        }

        // Log infos
        if !self.infos.is_empty() {
            info!("{} info messages:", self.infos.len());
            for info_msg in &self.infos {
                info!("  ℹ {}", info_msg);
            }
        }

        info!("=== End Validation Report ===");
    }

    /// Convert to Result - Err if validation failed
    pub fn into_result(self) -> Result<(), String> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(format!(
                "AppState validation failed with {} error(s): {}",
                self.errors.len(),
                self.errors.join("; ")
            ))
        }
    }
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to get environment variable as bool
pub fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(default)
}

/// Helper to check if environment variable is set and non-empty
pub fn env_is_set(key: &str) -> bool {
    std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_report_success() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "test_actor".to_string(),
            expected: true,
            present: true,
            severity: Severity::Critical,
            reason: "Required for operation".to_string(),
        });

        assert!(report.is_valid());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_validation_report_critical_error() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "critical_actor".to_string(),
            expected: true,
            present: false,
            severity: Severity::Critical,
            reason: "Required for operation".to_string(),
        });

        assert!(!report.is_valid());
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn test_validation_report_warning() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "optional_actor".to_string(),
            expected: true,
            present: false,
            severity: Severity::Warning,
            reason: "Optional feature".to_string(),
        });

        assert!(report.is_valid()); // Warnings don't fail validation
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn test_validation_report_info_severity_does_not_fail() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "info_actor".to_string(),
            expected: true,
            present: false,
            severity: Severity::Info,
            reason: "Informational".to_string(),
        });
        assert!(report.is_valid());
        assert_eq!(report.infos.len(), 1);
    }

    #[test]
    fn test_unexpected_present_item_goes_to_infos() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "surprise_actor".to_string(),
            expected: false,
            present: true,
            severity: Severity::Info,
            reason: "".to_string(),
        });
        assert!(report.is_valid());
        assert_eq!(report.infos.len(), 1);
        assert!(report.infos[0].contains("surprise_actor"));
    }

    #[test]
    fn test_into_result_ok_when_valid() {
        let report = ValidationReport::new();
        assert!(report.into_result().is_ok());
    }

    #[test]
    fn test_into_result_err_when_critical_error() {
        let mut report = ValidationReport::new();
        report.add(ValidationItem {
            name: "bad_actor".to_string(),
            expected: true,
            present: false,
            severity: Severity::Critical,
            reason: "required".to_string(),
        });
        let result = report.into_result();
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("bad_actor") || msg.contains("1 error"));
    }

    #[test]
    fn test_env_bool_returns_default_when_unset() {
        let unique_key = "VISIONCLAW_TEST_BOOL_UNSET_12345";
        std::env::remove_var(unique_key);
        assert!(!env_bool(unique_key, false));
        assert!(env_bool(unique_key, true));
    }

    #[test]
    fn test_env_bool_parses_true_string() {
        let key = "VISIONCLAW_TEST_BOOL_TRUE_12345";
        std::env::set_var(key, "true");
        assert!(env_bool(key, false));
        std::env::remove_var(key);
    }

    #[test]
    fn test_env_bool_parses_false_string() {
        let key = "VISIONCLAW_TEST_BOOL_FALSE_12345";
        std::env::set_var(key, "false");
        assert!(!env_bool(key, true));
        std::env::remove_var(key);
    }

    #[test]
    fn test_env_is_set_returns_false_when_unset() {
        let key = "VISIONCLAW_TEST_IS_SET_UNSET_12345";
        std::env::remove_var(key);
        assert!(!env_is_set(key));
    }

    #[test]
    fn test_env_is_set_returns_true_when_non_empty() {
        let key = "VISIONCLAW_TEST_IS_SET_NONEMPTY_12345";
        std::env::set_var(key, "anything");
        assert!(env_is_set(key));
        std::env::remove_var(key);
    }

    #[test]
    fn test_env_is_set_returns_false_for_empty_value() {
        let key = "VISIONCLAW_TEST_IS_SET_EMPTY_12345";
        std::env::set_var(key, "");
        assert!(!env_is_set(key));
        std::env::remove_var(key);
    }

    #[test]
    fn test_multiple_critical_errors_all_captured() {
        let mut report = ValidationReport::new();
        for i in 0..3 {
            report.add(ValidationItem {
                name: format!("actor_{}", i),
                expected: true,
                present: false,
                severity: Severity::Critical,
                reason: "required".to_string(),
            });
        }
        assert!(!report.is_valid());
        assert_eq!(report.errors.len(), 3);
    }
}
