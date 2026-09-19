use std::fmt::Write;

use crate::analyze::{flatten, row_estimate_ratio, worst_by_self_time};
use crate::model::PlanNode;

/// A row-estimate ratio outside this range is worth flagging — inside it
/// is normal planner slop, not a real problem.
const ESTIMATE_MISMATCH_THRESHOLD: f64 = 10.0;

pub fn render(root: &PlanNode) -> String {
    let mut out = String::new();

    let worst = worst_by_self_time(root, 3);
    if worst.iter().any(|n| n.actual_total_time.is_some()) {
        writeln!(out, "slowest node(s) by self-time:").ok();
        for node in &worst {
            let label = node
                .relation_name
                .as_deref()
                .map(|r| format!("{} on {r}", node.node_type))
                .unwrap_or_else(|| node.node_type.clone());
            writeln!(out, "  {label}").ok();
        }
        writeln!(out).ok();
    }

    for flat in flatten(root) {
        let indent = "  ".repeat(flat.depth);
        let label = flat
            .node
            .relation_name
            .as_deref()
            .map(|r| format!("{} on {r}", flat.node.node_type))
            .unwrap_or_else(|| flat.node.node_type.clone());

        let mut annotations = Vec::new();
        if let Some(actual_time) = flat.node.actual_total_time {
            annotations.push(format!("actual={actual_time:.2}ms"));
            annotations.push(format!("self={:.2}ms", flat.self_time));
        }
        if let Some(ratio) = row_estimate_ratio(flat.node) {
            if !(1.0 / ESTIMATE_MISMATCH_THRESHOLD..=ESTIMATE_MISMATCH_THRESHOLD).contains(&ratio) {
                annotations.push(format!(
                    "⚠ estimate off {ratio:.1}x (planned {:.0}, actual {:.0})",
                    flat.node.plan_rows,
                    flat.node.actual_rows.unwrap_or(0.0)
                ));
            }
        }

        if annotations.is_empty() {
            writeln!(out, "{indent}{label}").ok();
        } else {
            writeln!(out, "{indent}{label}  [{}]", annotations.join(", ")).ok();
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        node_type: &str,
        relation: Option<&str>,
        total_time: Option<f64>,
        plan_rows: f64,
        actual_rows: Option<f64>,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type: node_type.to_string(),
            relation_name: relation.map(str::to_string),
            total_cost: 0.0,
            plan_rows,
            actual_total_time: total_time,
            actual_rows,
            actual_loops: Some(1.0),
            children,
        }
    }

    #[test]
    fn renders_relation_name_when_present() {
        let root = node(
            "Seq Scan",
            Some("orders"),
            Some(10.0),
            100.0,
            Some(100.0),
            vec![],
        );
        let text = render(&root);
        assert!(text.contains("Seq Scan on orders"));
    }

    #[test]
    fn indents_children_deeper_than_parents() {
        let child = node(
            "Index Scan",
            Some("users"),
            Some(1.0),
            1.0,
            Some(1.0),
            vec![],
        );
        let root = node("Nested Loop", None, Some(5.0), 1.0, Some(1.0), vec![child]);
        let text = render(&root);
        let child_line = text.lines().find(|l| l.contains("Index Scan")).unwrap();
        assert!(
            child_line.starts_with("  "),
            "child must be indented relative to the root"
        );
    }

    #[test]
    fn flags_a_large_row_estimate_mismatch() {
        let root = node(
            "Seq Scan",
            Some("orders"),
            Some(10.0),
            10.0,
            Some(5000.0),
            vec![],
        );
        let text = render(&root);
        assert!(text.contains("estimate off"));
    }

    #[test]
    fn does_not_flag_a_reasonable_row_estimate() {
        let root = node(
            "Seq Scan",
            Some("orders"),
            Some(10.0),
            100.0,
            Some(110.0),
            vec![],
        );
        let text = render(&root);
        assert!(!text.contains("estimate off"));
    }

    #[test]
    fn a_plan_without_analyze_data_still_renders_without_panicking() {
        let root = PlanNode {
            node_type: "Seq Scan".to_string(),
            relation_name: Some("orders".to_string()),
            total_cost: 10.0,
            plan_rows: 100.0,
            actual_total_time: None,
            actual_rows: None,
            actual_loops: None,
            children: vec![],
        };
        let text = render(&root);
        assert!(text.contains("Seq Scan on orders"));
        assert!(
            !text.contains("slowest node"),
            "no ANALYZE data means no self-time summary to show"
        );
    }
}
