// src/cqrs/types.rs
//! CQRS Base Types and Traits
//!
//! Defines the fundamental traits for the Command Query Responsibility Segregation pattern.
//! Commands represent write operations, Queries represent read operations.

use async_trait::async_trait;
use std::fmt::Debug;

pub type Result<T> = anyhow::Result<T>;

pub trait Command: Send + Sync + Debug {
    
    type Result: Send;

    
    fn name(&self) -> &'static str;

    
    
    
    
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

pub trait Query: Send + Sync + Debug {
    
    type Result: Send;

    
    fn name(&self) -> &'static str;

    
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
pub trait CommandHandler<C: Command>: Send + Sync {
    
    
    
    
    async fn handle(&self, command: C) -> Result<C::Result>;
}

#[async_trait]
pub trait QueryHandler<Q: Query>: Send + Sync {
    
    
    
    
    async fn handle(&self, query: Q) -> Result<Q::Result>;
}

#[async_trait]
#[allow(unused_variables)]
pub trait CommandMiddleware: Send + Sync {
    
    async fn before_execute(&self, _command_name: &str) -> Result<()> {
        Ok(())
    }


    async fn after_execute(&self, _command_name: &str) -> Result<()> {
        Ok(())
    }


    async fn on_error(&self, _command_name: &str, _error: &anyhow::Error) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
#[allow(unused_variables)]
pub trait QueryMiddleware: Send + Sync {

    async fn before_execute(&self, _query_name: &str) -> Result<()> {
        Ok(())
    }


    async fn after_execute(&self, _query_name: &str) -> Result<()> {
        Ok(())
    }


    async fn on_error(&self, _query_name: &str, _error: &anyhow::Error) -> Result<()> {
        Ok(())
    }
}

pub struct LoggingMiddleware;

#[async_trait]
impl CommandMiddleware for LoggingMiddleware {
    async fn before_execute(&self, command_name: &str) -> Result<()> {
        tracing::info!(command = %command_name, "Executing command");
        Ok(())
    }

    async fn after_execute(&self, command_name: &str) -> Result<()> {
        tracing::info!(command = %command_name, "Command executed successfully");
        Ok(())
    }

    async fn on_error(&self, command_name: &str, error: &anyhow::Error) -> Result<()> {
        tracing::error!(command = %command_name, error = %error, "Command execution failed");
        Ok(())
    }
}

#[async_trait]
impl QueryMiddleware for LoggingMiddleware {
    async fn before_execute(&self, query_name: &str) -> Result<()> {
        tracing::debug!(query = %query_name, "Executing query");
        Ok(())
    }

    async fn after_execute(&self, query_name: &str) -> Result<()> {
        tracing::debug!(query = %query_name, "Query executed successfully");
        Ok(())
    }

    async fn on_error(&self, query_name: &str, error: &anyhow::Error) -> Result<()> {
        tracing::warn!(query = %query_name, error = %error, "Query execution failed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestCommand {
        value: String,
    }

    impl Command for TestCommand {
        type Result = ();

        fn name(&self) -> &'static str {
            "TestCommand"
        }

        fn validate(&self) -> Result<()> {
            if self.value.is_empty() {
                return Err(anyhow::anyhow!("Value cannot be empty"));
            }
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct TestQuery {
        id: u32,
    }

    impl Query for TestQuery {
        type Result = String;

        fn name(&self) -> &'static str {
            "TestQuery"
        }
    }

    #[test]
    fn test_command_validation() {
        let valid = TestCommand {
            value: "test".to_string(),
        };
        assert!(valid.validate().is_ok());

        let invalid = TestCommand {
            value: "".to_string(),
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_command_name() {
        let cmd = TestCommand {
            value: "test".to_string(),
        };
        assert_eq!(cmd.name(), "TestCommand");
    }

    #[test]
    fn test_query_name() {
        let query = TestQuery { id: 1 };
        assert_eq!(query.name(), "TestQuery");
    }
}
