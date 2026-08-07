use regex::Regex;

use crate::domain::{ParameterType, TaskDefinition, TaskRequest};
use crate::error::{AppError, AppResult, ErrorCode};

pub trait RequestValidator {
    fn validate(&self, request: &TaskRequest, definition: &TaskDefinition) -> AppResult<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultRequestValidator;

impl RequestValidator for DefaultRequestValidator {
    fn validate(&self, request: &TaskRequest, definition: &TaskDefinition) -> AppResult<()> {
        validate_task_request(request, definition)
    }
}

pub fn validate_task_request(request: &TaskRequest, definition: &TaskDefinition) -> AppResult<()> {
    for name in request.parameters.keys() {
        if !definition.parameters.contains_key(name) {
            return Err(AppError::new(
                ErrorCode::InvalidParameter,
                format!("unknown parameter: {name}"),
            ));
        }
    }

    for (name, parameter) in &definition.parameters {
        let value = request.parameters.get(name);

        if parameter.required && value.is_none() {
            return Err(AppError::new(
                ErrorCode::InvalidParameter,
                format!("missing required parameter: {name}"),
            ));
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
            return Err(AppError::new(
                ErrorCode::InvalidParameter,
                format!("parameter {name} has invalid type"),
            ));
        }

        if let Some(pattern) = &parameter.pattern {
            if parameter.kind != ParameterType::String {
                return Err(AppError::new(
                    ErrorCode::InvalidTaskDefinition,
                    format!("parameter {name} uses a pattern but is not a string"),
                ));
            }

            let candidate = value.as_str().ok_or_else(|| {
                AppError::new(
                    ErrorCode::InvalidParameter,
                    format!("parameter {name} must be a string"),
                )
            })?;
            let regex = Regex::new(pattern).map_err(|_| {
                AppError::new(
                    ErrorCode::InvalidTaskDefinition,
                    format!("invalid validation pattern for parameter {name}"),
                )
            })?;

            if !regex.is_match(candidate) {
                return Err(AppError::new(
                    ErrorCode::InvalidParameter,
                    format!("parameter {name} does not match the allowed pattern"),
                ));
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
    fn rejects_missing_required_parameter_with_stable_code() {
        let error = validate_task_request(&request(BTreeMap::new()), &definition()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParameter);
    }

    #[test]
    fn rejects_unknown_parameter() {
        let request = request(BTreeMap::from([
            ("service".to_owned(), json!("demo")),
            ("shell".to_owned(), json!("rm -rf /")),
        ]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParameter);
    }

    #[test]
    fn rejects_wrong_type() {
        let request = request(BTreeMap::from([("service".to_owned(), json!(123))]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParameter);
    }

    #[test]
    fn rejects_pattern_violation() {
        let request = request(BTreeMap::from([(
            "service".to_owned(),
            json!("demo; shutdown -h now"),
        )]));
        let error = validate_task_request(&request, &definition()).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidParameter);
    }
}
