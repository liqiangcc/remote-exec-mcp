use anyhow::{bail, Context, Result};
use regex::Regex;

use crate::domain::{ParameterType, TaskDefinition, TaskRequest};

pub trait RequestValidator {
    fn validate(&self, request: &TaskRequest, definition: &TaskDefinition) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultRequestValidator;

impl RequestValidator for DefaultRequestValidator {
    fn validate(&self, request: &TaskRequest, definition: &TaskDefinition) -> Result<()> {
        validate_task_request(request, definition)
    }
}

pub fn validate_task_request(request: &TaskRequest, definition: &TaskDefinition) -> Result<()> {
    for name in request.parameters.keys() {
        if !definition.parameters.contains_key(name) {
            bail!("unknown parameter: {name}");
        }
    }

    for (name, parameter) in &definition.parameters {
        let value = request.parameters.get(name);

        if parameter.required && value.is_none() {
            bail!("missing required parameter: {name}");
        }

        let Some(value) = value else {
            continue;
        };

        let type_matches = match parameter.kind {
            ParameterType::String => value.is_string(),
            ParameterType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
            ParameterType::Boolean => value.is_boolean(),
        };

        if !type_matches {
            bail!("parameter {name} has invalid type");
        }

        if let Some(pattern) = &parameter.pattern {
            if parameter.kind != ParameterType::String {
                bail!("parameter {name} uses a pattern but is not a string");
            }

            let candidate = value
                .as_str()
                .with_context(|| format!("parameter {name} must be a string"))?;
            let regex = Regex::new(pattern)
                .with_context(|| format!("invalid validation pattern for parameter {name}"))?;

            if !regex.is_match(candidate) {
                bail!("parameter {name} does not match the allowed pattern");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::domain::{ParameterDefinition, TargetId};

    fn definition() -> TaskDefinition {
        TaskDefinition {
            name: "service-status".to_owned(),
            description: None,
            parameters: BTreeMap::from([
                (
                    "service".to_owned(),
                    ParameterDefinition {
                        kind: ParameterType::String,
                        pattern: Some("^[a-zA-Z0-9_.@-]+$".to_owned()),
                        required: true,
                    },
                ),
                (
                    "force".to_owned(),
                    ParameterDefinition {
                        kind: ParameterType::Boolean,
                        pattern: None,
                        required: false,
                    },
                ),
            ]),
            timeout_seconds: 15,
        }
    }

    fn request(parameters: BTreeMap<String, serde_json::Value>) -> TaskRequest {
        TaskRequest {
            target: TargetId("local-test".to_owned()),
            task: "service-status".to_owned(),
            parameters,
        }
    }

    #[test]
    fn accepts_valid_parameters() {
        let request = request(BTreeMap::from([
            ("service".to_owned(), json!("demo.service")),
            ("force".to_owned(), json!(false)),
        ]));

        assert!(validate_task_request(&request, &definition()).is_ok());
    }

    #[test]
    fn rejects_missing_required_parameter() {
        let error = validate_task_request(&request(BTreeMap::new()), &definition()).unwrap_err();
        assert!(error.to_string().contains("missing required parameter"));
    }

    #[test]
    fn rejects_unknown_parameter() {
        let request = request(BTreeMap::from([
            ("service".to_owned(), json!("demo")),
            ("shell".to_owned(), json!("rm -rf /")),
        ]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert!(error.to_string().contains("unknown parameter"));
    }

    #[test]
    fn rejects_wrong_type() {
        let request = request(BTreeMap::from([("service".to_owned(), json!(123))]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert!(error.to_string().contains("invalid type"));
    }

    #[test]
    fn rejects_pattern_violation() {
        let request = request(BTreeMap::from([(
            "service".to_owned(),
            json!("demo; shutdown -h now"),
        )]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert!(error.to_string().contains("allowed pattern"));
    }
}
