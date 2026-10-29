//! Regressions for the ready library's decision fan-out routing backport.
use mermaid_rs_renderer::{Layout, LayoutConfig, Theme, compute_layout, parse_mermaid_strict};

const EPS: f32 = 0.55;

fn layout(source: &str) -> Layout {
    let parsed = parse_mermaid_strict(source).unwrap();
    compute_layout(&parsed.graph, &Theme::modern(), &LayoutConfig::default())
}

fn preview(direction: &str) -> Layout {
    layout(&format!(
        "flowchart {direction}\n A[Сообщение] --> B[Markdown]\n B --> C{{Блок}}\n C -->|mermaid| D[Диаграмма]\n C -->|latex| E[Формула]"
    ))
}

fn target_center(layout: &Layout, direction: &str, target: &str) -> (f32, f32) {
    let n = &layout.nodes[target];
    match direction {
        "LR" => (n.x, n.y + n.height / 2.0),
        "RL" => (n.x + n.width, n.y + n.height / 2.0),
        "TB" => (n.x + n.width / 2.0, n.y),
        "BT" => (n.x + n.width / 2.0, n.y + n.height),
        _ => unreachable!(),
    }
}

fn close(actual: (f32, f32), expected: (f32, f32), context: &str) {
    assert!(
        (actual.0 - expected.0).abs() <= EPS && (actual.1 - expected.1).abs() <= EPS,
        "{context}: {actual:?} != {expected:?}"
    );
}

#[test]
fn decision_preview_rotates_target_face_centers_and_terminal_normals() {
    for direction in ["LR", "RL", "TB", "BT"] {
        let layout = preview(direction);
        assert_routes_avoid_node_interiors(&layout);
        let mut starts = Vec::new();
        for edge in layout.edges.iter().filter(|edge| edge.from == "C") {
            starts.push(edge.points[0]);
            for segment in edge.points.windows(2) {
                assert!(
                    (segment[0].0 - segment[1].0).abs() <= EPS
                        || (segment[0].1 - segment[1].1).abs() <= EPS,
                    "{direction}: diagonal fan segment {:?}",
                    edge.points
                );
            }
            let end = *edge.points.last().unwrap();
            let prev = edge.points[edge.points.len() - 2];
            close(end, target_center(&layout, direction, &edge.to), direction);
            if matches!(direction, "LR" | "RL") {
                assert!(
                    (prev.1 - end.1).abs() <= EPS,
                    "{direction}: diagonal target entry {:?}",
                    edge.points
                );
            } else {
                assert!(
                    (prev.0 - end.0).abs() <= EPS,
                    "{direction}: diagonal target entry {:?}",
                    edge.points
                );
            }
        }
        let n = &layout.nodes["C"];
        let center = (n.x + n.width / 2.0, n.y + n.height / 2.0);
        close(
            (
                (starts[0].0 + starts[1].0) / 2.0,
                (starts[0].1 + starts[1].1) / 2.0,
            ),
            center,
            "symmetric fan exits",
        );
        assert!(
            (starts[0].0 - starts[1].0).abs() + (starts[0].1 - starts[1].1).abs()
                > n.width.min(n.height) * 0.9
        );
    }
}

fn assert_routes_avoid_node_interiors(layout: &Layout) {
    use mermaid_rs_renderer::NodeShape;
    for edge in &layout.edges {
        for segment in edge.points.windows(2) {
            let (a, b) = (segment[0], segment[1]);
            let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / 0.5).ceil() as usize;
            for i in 0..=steps.max(1) {
                let t = i as f32 / steps.max(1) as f32;
                let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                for node in layout
                    .nodes
                    .values()
                    .filter(|node| !node.hidden && node.anchor_subgraph.is_none())
                {
                    let inside = if node.shape == NodeShape::Diamond {
                        ((p.0 - node.x - node.width / 2.0) / (node.width / 2.0)).abs()
                            + ((p.1 - node.y - node.height / 2.0) / (node.height / 2.0)).abs()
                            < 0.98
                    } else {
                        p.0 > node.x + EPS
                            && p.0 < node.x + node.width - EPS
                            && p.1 > node.y + EPS
                            && p.1 < node.y + node.height - EPS
                    };
                    assert!(
                        !inside,
                        "{} -> {} crosses {} at {p:?}: {:?}",
                        edge.from, edge.to, node.id, edge.points
                    );
                }
            }
        }
    }
}

#[test]
fn branching_routes_avoid_intervening_nodes_and_place_labels() {
    for direction in ["LR", "RL", "TB", "BT"] {
        let layout = layout(&format!(
            "flowchart {direction}\n A{{Choice}} -->|left| B[First]\n A -->|right| C[Second]\n B --> D[Join]\n C --> D\n A -->|skip| D"
        ));
        assert_routes_avoid_node_interiors(&layout);
        for edge in layout.edges.iter().filter(|edge| edge.label.is_some()) {
            let label = edge.label.as_ref().unwrap();
            let (x, y) = edge.label_anchor.expect("library places edge label");
            assert!(x.is_finite() && y.is_finite());
            for node in layout.nodes.values() {
                assert!(
                    x + label.width / 2.0 <= node.x + EPS
                        || x - label.width / 2.0 >= node.x + node.width - EPS
                        || y + label.height / 2.0 <= node.y + EPS
                        || y - label.height / 2.0 >= node.y + node.height - EPS,
                    "{direction}: label {:?} overlaps {}",
                    label.lines,
                    node.id
                );
            }
        }
    }
}

#[test]
fn short_chains_loops_back_edges_and_parallel_branches_keep_valid_routes() {
    for direction in ["LR", "RL", "TB", "BT"] {
        for body in [
            "A --> B --> C",
            "A{Choice} --> B[Only]",
            "A{Choice} -->|one| B[Target]\n A -->|two| B\n A --> C[Other]",
            "A{Choice} --> B[Target]\n A --> C[Other]\n B --> A\n C --> C",
            "subgraph S\n A{Choice}\n end\n A --> B[Outside]\n A --> C[Other]",
        ] {
            let layout = layout(&format!("flowchart {direction}\n{body}"));
            assert!(!layout.edges.is_empty());
            for edge in &layout.edges {
                assert!(edge.points.len() >= 2);
                assert!(
                    edge.points
                        .iter()
                        .all(|(x, y)| x.is_finite() && y.is_finite())
                );
            }
            assert_routes_avoid_node_interiors(&layout);
            if body.contains("one") {
                let parallel: Vec<_> = layout
                    .edges
                    .iter()
                    .filter(|edge| edge.from == "A" && edge.to == "B")
                    .collect();
                assert_eq!(parallel.len(), 2);
                assert_ne!(
                    parallel[0].points, parallel[1].points,
                    "parallel lanes must remain distinct"
                );
            }
        }
    }
}
