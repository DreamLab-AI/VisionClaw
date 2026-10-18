//! Middleware modules for request processing

pub mod auth;
pub mod public_demo;
pub mod rate_limit;
pub mod rbac_gate;
pub mod timeout;

pub use auth::{get_authenticated_user, AuthenticatedUser, RequireAuth};
pub use public_demo::PublicDemoGuard;
pub use rate_limit::{RateLimit, RateLimitConfig};
pub use rbac_gate::RbacGate;
pub use timeout::{TimeoutConfig, TimeoutMiddleware};
