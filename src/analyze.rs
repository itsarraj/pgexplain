use crate::model::PlanNode;

/// `Actual Total Time` is Postgres's **per-loop average**, not the node's
/// real total wall-clock contribution — a node executed 1000 times inside
/// a nested loop join reports the average single-execution time, not
/// 1000x that. Multiplying by `Actual Loops` is what `explain.depesz.com`
/// itself does and is the actual point of this function: without it,
/// self-time below would be wildly wrong for any plan with a nested loop.
pub fn effective_total_time(node: &PlanNode) -> f64 {
    match (node.actual_total_time, node.actual_loops) {
        (Some(t), Some(loops)) => t * loops,
        (Some(t), None) => t,
        (None, _) => 0.0,
    }
}

/// This node's own time, excluding time already accounted for by its
/// children — the actual "what's slow" signal. A node with a huge
/// `effective_total_time` but almost all of it spent in a child isn't
/// the problem; a node whose *self* time is huge is.
pub fn self_time(node: &PlanNode) -> f64 {
    let children_total: f64 = node.children.iter().map(effective_total_time).sum();
    (effective_total_time(node) - children_total).max(0.0)
}

/// `actual / planned` row count — far from 1.0 in either direction means
/// the planner's statistics are stale or the query has a correlation the
/// planner can't see, both real, actionable signals. `None` when there's
/// no `Actual Rows` (not an `ANALYZE` run) or `Plan Rows` is zero (can't
/// meaningfully divide).
pub fn row_estimate_ratio(node: &PlanNode) -> Option<f64> {
    let actual = node.actual_rows?;
    if node.plan_rows <= 0.0 {
        return None;
    }
    Some(actual / node.plan_rows)
}

pub struct FlatNode<'a> {
    pub node: &'a PlanNode,
    pub depth: usize,
    pub self_time: f64,
}

fn flatten_into<'a>(node: &'a PlanNode, depth: usize, out: &mut Vec<FlatNode<'a>>) {
    out.push(FlatNode {
        node,
        depth,
        self_time: self_time(node),
    });
    for child in &node.children {
        flatten_into(child, depth + 1, out);
    }
}

/// The whole plan tree as a flat, depth-annotated list in original
/// (pre-order) tree order — what `render.rs` walks to print the indented
/// tree.
pub fn flatten(root: &PlanNode) -> Vec<FlatNode<'_>> {
    let mut result = Vec::new();
    flatten_into(root, 0, &mut result);
    result
}

/// The `top_n` nodes by self-time, descending — the actual answer to
/// "what's slow," independent of where each node sits in the tree.
pub fn worst_by_self_time(root: &PlanNode, top_n: usize) -> Vec<&PlanNode> {
    let mut flat = flatten(root);
    flat.sort_by(|a, b| {
        b.self_time
            .partial_cmp(&a.self_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    flat.into_iter().take(top_n).map(|f| f.node).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(
        node_type: &str,
        total_time: Option<f64>,
        loops: Option<f64>,
        plan_rows: f64,
        actual_rows: Option<f64>,
        children: Vec<PlanNode>,
    ) -> PlanNode {
        PlanNode {
            node_type: node_type.to_string(),
            relation_name: None,
            total_cost: 0.0,
            plan_rows,
            actual_total_time: total_time,
            actual_rows,
            actual_loops: loops,
            children,
        }
    }

    #[test]
    fn effective_total_time_multiplies_by_loops() {
        let n = node(
            "Index Scan",
            Some(2.0),
            Some(1000.0),
            1000.0,
            Some(1000.0),
            vec![],
        );
        assert_eq!(effective_total_time(&n), 2000.0);
    }

    #[test]
    fn self_time_excludes_children() {
        let child = node(
            "Seq Scan",
            Some(30.0),
            Some(1.0),
            100.0,
            Some(100.0),
            vec![],
        );
        let parent = node(
            "Sort",
            Some(50.0),
            Some(1.0),
            100.0,
            Some(100.0),
            vec![child],
        );
        // Parent's cumulative 50ms includes the child's 30ms; self-time is 20ms.
        assert_eq!(self_time(&parent), 20.0);
    }

    #[test]
    fn self_time_never_goes_negative_from_floating_point_noise() {
        let child = node(
            "Seq Scan",
            Some(30.0000001),
            Some(1.0),
            100.0,
            Some(100.0),
            vec![],
        );
        let parent = node(
            "Sort",
            Some(30.0),
            Some(1.0),
            100.0,
            Some(100.0),
            vec![child],
        );
        assert_eq!(self_time(&parent), 0.0);
    }

    #[test]
    fn row_estimate_ratio_flags_a_bad_estimate() {
        let n = node("Seq Scan", Some(1.0), Some(1.0), 10.0, Some(5000.0), vec![]);
        let ratio = row_estimate_ratio(&n).unwrap();
        assert!(
            ratio > 400.0,
            "planner expected 10 rows, got 5000 — a real stale-statistics signal"
        );
    }

    #[test]
    fn worst_by_self_time_finds_the_actually_slow_node_not_the_root() {
        // The root's cumulative time is dominated by a slow child; the
        // root's own self-time is small. worst_by_self_time must surface
        // the child, not just report the root because it has the biggest
        // *cumulative* number.
        let slow_child = node(
            "Seq Scan",
            Some(500.0),
            Some(1.0),
            1000.0,
            Some(1000.0),
            vec![],
        );
        let fast_child = node("Index Scan", Some(2.0), Some(1.0), 10.0, Some(10.0), vec![]);
        let root = node(
            "Hash Join",
            Some(505.0),
            Some(1.0),
            1000.0,
            Some(1000.0),
            vec![slow_child, fast_child],
        );

        let worst = worst_by_self_time(&root, 1);
        assert_eq!(worst[0].node_type, "Seq Scan");
    }
}
