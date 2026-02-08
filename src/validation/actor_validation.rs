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
            info!("✅ Validated {} components:", successful.len());
            for item in successful {
                info!("  ✓ {}: {}", item.name, item.reason);
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
            info!("ℹ️  {} info messages:", self.infos.len());
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

/// Helper to check if a feature is enabled
pub fn is_feature_enabled(feature: &str) -> bool {
    match feature {
        "gpu" => cfg!(feature = "gpu"),
        "ontology" => cfg!(feature = "ontology"),
        _ => false,
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
}
