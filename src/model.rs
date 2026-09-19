use serde::Deserialize;

/// Mirrors the JSON keys `EXPLAIN (ANALYZE, FORMAT JSON)` actually emits
/// — Postgres's own field names (`"Node Type"`, `"Actual Total Time"`,
/// ...), not renamed to Rust convention until they land in this struct.
/// Every numeric field the `ANALYZE` variant doesn't always populate
/// (e.g. a plan run without `ANALYZE` has no `Actual *` fields at all)
/// is `Option`, so parsing a plain (non-`ANALYZE`) explain doesn't fail,
/// it just has less to show.
#[derive(Debug, Clone, Deserialize)]
pub struct PlanNode {
    #[serde(rename = "Node Type")]
    pub node_type: String,
    #[serde(rename = "Relation Name")]
    pub relation_name: Option<String>,
    #[serde(rename = "Total Cost")]
    pub total_cost: f64,
    #[serde(rename = "Plan Rows")]
    pub plan_rows: f64,
    #[serde(rename = "Actual Total Time")]
    pub actual_total_time: Option<f64>,
    #[serde(rename = "Actual Rows")]
    pub actual_rows: Option<f64>,
    #[serde(rename = "Actual Loops")]
    pub actual_loops: Option<f64>,
    #[serde(rename = "Plans", default)]
    pub children: Vec<PlanNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExplainEntry {
    #[serde(rename = "Plan")]
    plan: PlanNode,
}

/// Parses the full `EXPLAIN (FORMAT JSON)` output — a JSON **array**
/// containing exactly one object with a `"Plan"` key (that's the actual
/// top-level shape Postgres emits, easy to miss if you only look at the
/// inner plan tree in documentation examples) — and returns the root
/// plan node.
pub fn parse_explain_json(json: &str) -> Result<PlanNode, String> {
    let entries: Vec<ExplainEntry> = serde_json::from_str(json)
        .map_err(|e| format!("not valid EXPLAIN (FORMAT JSON) output: {e}"))?;
    entries
        .into_iter()
        .next()
        .map(|e| e.plan)
        .ok_or_else(|| "empty explain output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        {
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "orders",
                "Total Cost": 1234.5,
                "Plan Rows": 1000,
                "Actual Total Time": 42.3,
                "Actual Rows": 950,
                "Actual Loops": 1,
                "Plans": [
                    {
                        "Node Type": "Sort",
                        "Total Cost": 100.0,
                        "Plan Rows": 100,
                        "Actual Total Time": 10.0,
                        "Actual Rows": 100,
                        "Actual Loops": 1
                    }
                ]
            }
        }
    ]"#;

    #[test]
    fn parses_the_real_array_wrapped_explain_shape() {
        let plan = parse_explain_json(SAMPLE).unwrap();
        assert_eq!(plan.node_type, "Seq Scan");
        assert_eq!(plan.relation_name.as_deref(), Some("orders"));
        assert_eq!(plan.actual_total_time, Some(42.3));
        assert_eq!(plan.children.len(), 1);
        assert_eq!(plan.children[0].node_type, "Sort");
    }

    #[test]
    fn plan_without_analyze_fields_still_parses() {
        let json = r#"[{"Plan": {"Node Type": "Seq Scan", "Total Cost": 10.0, "Plan Rows": 5}}]"#;
        let plan = parse_explain_json(json).unwrap();
        assert_eq!(plan.actual_total_time, None);
        assert!(plan.children.is_empty());
    }

    #[test]
    fn invalid_json_is_a_clean_error() {
        assert!(parse_explain_json("not json").is_err());
    }

    #[test]
    fn empty_array_is_a_clean_error() {
        assert!(parse_explain_json("[]").is_err());
    }
}
