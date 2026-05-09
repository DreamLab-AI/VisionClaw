//! Middleware modules for request processing

pub mod auth;
pub mod enterprise_auth;
pub mod timeout;
pub mod validation;

pub use auth::{get_authenticated_user, AuthenticatedUser, RequireAuth};
pub use enterprise_auth::{
    get_enterprise_role, require_role, EnterpriseRoleExt, RequireRole,
};
// Rate limiting re-exported from the canonical token-bucket implementation
// (ADR-087 D2: superseded fixed-window middleware::rate_limit deleted)
pub use crate::utils::validation::middleware::RateLimit;
pub use crate::utils::validation::rate_limit::RateLimitConfig;
pub use timeout::TimeoutMiddleware;
pub use validation::{ValidateInput, ValidationConfig, validators};
