use crate::domain::target::Target;

/// MCP tool adapter for listing available targets.
///
/// This layer only converts application data into MCP responses.
/// It does not perform authorization or access SSH directly.
pub struct ListTargetsTool;

impl ListTargetsTool {
    pub fn name() -> &'static str {
        "list_targets"
    }

    pub fn execute(targets: Vec<Target>) -> Vec<TargetSummary> {
        targets
            .into_iter()
            .map(|target| TargetSummary {
                name: target.name,
                description: target.description,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct TargetSummary {
    pub name: String,
    pub description: Option<String>,
}
