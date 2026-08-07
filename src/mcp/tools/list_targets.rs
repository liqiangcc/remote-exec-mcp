use serde::Serialize;

use crate::catalog::Catalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetSummary {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListTargetsResponse {
    pub targets: Vec<TargetSummary>,
}

/// MCP adapter for target discovery.
///
/// Discovery data comes from the application catalog. The protocol layer does
/// not read configuration or probe SSH endpoints directly.
pub fn list_targets<C>(catalog: &C) -> ListTargetsResponse
where
    C: Catalog,
{
    ListTargetsResponse {
        targets: catalog
            .list_targets()
            .into_iter()
            .map(|target| TargetSummary { name: target.0 })
            .collect(),
    }
}
