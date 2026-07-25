use std::env;

use config::builder::DefaultState;
pub use config::{Config, ConfigBuilder, ConfigError, Environment, File};

#[derive(Debug, Clone)]
pub struct Profile {
    name: String,
}

impl Profile {
    pub fn from_environment(variable: &str, default: &str) -> Self {
        Self {
            name: env::var(variable).unwrap_or_else(|_| default.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn builder(
        &self,
        directory: &str,
        environment_prefix: &str,
    ) -> ConfigBuilder<DefaultState> {
        let file = format!("{directory}/{}", self.name);
        config::Config::builder()
            .add_source(File::with_name(&file).required(false))
            .add_source(
                Environment::with_prefix(environment_prefix)
                    .separator("__")
                    .prefix_separator("__"),
            )
    }
}

pub trait ValidateConfiguration {
    type Capabilities: Copy;

    fn validate_structure(&self) -> Result<(), String>;
    fn validate_capabilities(&self, capabilities: Self::Capabilities) -> Result<(), String>;
    fn validate_production_policy(&self) -> Result<(), String>;
}

pub fn validate<C>(
    configuration: &C,
    capabilities: C::Capabilities,
) -> Result<(), ConfigurationValidationError>
where
    C: ValidateConfiguration,
{
    configuration
        .validate_structure()
        .map_err(ConfigurationValidationError::Structure)?;
    configuration
        .validate_capabilities(capabilities)
        .map_err(ConfigurationValidationError::Capabilities)?;
    configuration
        .validate_production_policy()
        .map_err(ConfigurationValidationError::Production)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationValidationError {
    Structure(String),
    Capabilities(String),
    Production(String),
}

impl std::fmt::Display for ConfigurationValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structure(error) => {
                write!(
                    formatter,
                    "structurally invalid application configuration: {error}"
                )
            }
            Self::Capabilities(error) => {
                write!(formatter, "invalid application capabilities: {error}")
            }
            Self::Production(error) => {
                write!(
                    formatter,
                    "application configuration violates production policy: {error}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigurationValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestConfiguration {
        failures: [bool; 3],
    }

    impl ValidateConfiguration for TestConfiguration {
        type Capabilities = ();

        fn validate_structure(&self) -> Result<(), String> {
            (!self.failures[0])
                .then_some(())
                .ok_or_else(|| "structure".to_string())
        }

        fn validate_capabilities(&self, _capabilities: ()) -> Result<(), String> {
            (!self.failures[1])
                .then_some(())
                .ok_or_else(|| "capabilities".to_string())
        }

        fn validate_production_policy(&self) -> Result<(), String> {
            (!self.failures[2])
                .then_some(())
                .ok_or_else(|| "production".to_string())
        }
    }

    #[test]
    fn validation_fails_in_structural_capability_production_order() {
        assert!(matches!(
            validate(
                &TestConfiguration {
                    failures: [true, true, true],
                },
                ()
            ),
            Err(ConfigurationValidationError::Structure(_))
        ));
        assert!(matches!(
            validate(
                &TestConfiguration {
                    failures: [false, true, true],
                },
                ()
            ),
            Err(ConfigurationValidationError::Capabilities(_))
        ));
        assert!(matches!(
            validate(
                &TestConfiguration {
                    failures: [false, false, true],
                },
                ()
            ),
            Err(ConfigurationValidationError::Production(_))
        ));
    }
}
