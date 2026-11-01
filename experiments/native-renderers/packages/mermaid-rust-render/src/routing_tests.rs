//! Regressions for the ready router's opt-in orthogonal flowchart mode.
use mermaid_rs_renderer::{Layout, LayoutConfig, Theme, compute_layout, parse_mermaid_strict};

const EPS: f32 = 0.55;

fn layout(source: &str) -> Layout {
    let parsed = parse_mermaid_strict(source).unwrap();
    compute_layout(
        &parsed.graph,
        &Theme::modern(),
        &super::layout_config(parsed.graph.kind),
    )
}

fn assert_orthogonal(layout: &Layout) {
    for edge in &layout.edges {
        assert!(edge.points.len() >= 2);
        for segment in edge.points.windows(2) {
            assert!(
                (segment[0].0 - segment[1].0).abs() <= 0.001
                    || (segment[0].1 - segment[1].1).abs() <= 0.001,
                "{} -> {}: diagonal route {:?}",
                edge.from,
                edge.to,
                edge.points
            );
        }
    }
}

#[test]
fn ordinary_small_fan_uses_existing_orthogonal_candidates_without_changing_library_default() {
    let source = include_str!("../tests/fixtures/orthogonal-small-fan.mmd");
    let parsed = parse_mermaid_strict(source).unwrap();
    let default = LayoutConfig::default();
    assert!(!default.flowchart.routing.orthogonal_only);
    let upstream = compute_layout(&parsed.graph, &Theme::modern(), &default);
    assert!(
        upstream
            .edges
            .iter()
            .any(|edge| edge.points.windows(2).any(|segment| {
                (segment[0].0 - segment[1].0).abs() > 0.001
                    && (segment[0].1 - segment[1].1).abs() > 0.001
            })),
        "fixture must exercise the former diagonal shortcut"
    );
    for direction in ["LR", "RL", "TB", "BT"] {
        let rotated = source.replacen("flowchart LR", &format!("flowchart {direction}"), 1);
        let fixed = layout(&rotated);
        assert_orthogonal(&fixed);
        assert_routes_avoid_node_interiors(&fixed);
    }
}

#[test]
fn routing_mode_is_only_enabled_for_flowcharts() {
    for kind in [
        mermaid_rs_renderer::DiagramKind::Class,
        mermaid_rs_renderer::DiagramKind::Sequence,
        mermaid_rs_renderer::DiagramKind::State,
        mermaid_rs_renderer::DiagramKind::Er,
    ] {
        assert!(!super::layout_config(kind).flowchart.routing.orthogonal_only);
    }
}

#[test]
fn skip_link_removes_the_source_micro_hairpin_and_keeps_labels_clear() {
    let fixture = include_str!("../tests/fixtures/orthogonal-skip-link.mmd");
    for direction in ["LR", "RL", "TB", "BT"] {
        let source = fixture.replacen("flowchart TB", &format!("flowchart {direction}"), 1);
        let layout = layout(&source);
        assert_orthogonal(&layout);
        assert_routes_avoid_node_interiors(&layout);
        let edge = layout
            .edges
            .iter()
            .find(|edge| edge.from == "A" && edge.to == "E")
            .unwrap();
        assert_eq!(edge.label.as_ref().unwrap().lines.join(" "), "Быстрый путь");
        // This reported TB regression had a 1.6px reversed third segment;
        // unrelated larger routing detours are outside this local cleanup.
        if direction == "TB" {
            assert!(
                edge.points
                    .windows(2)
                    .take(3)
                    .all(|s| s[1].1 >= s[0].1 - 0.001),
                "{direction}: skip-link backtracks: {:?}",
                edge.points
            );
        }
        for edge in &layout.edges {
            if let Some(label) = &edge.label {
                let (x, y) = edge.label_anchor.unwrap();
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
}

#[test]
fn diamond_fan_in_uses_one_axis_vertex_in_all_directions_and_rounded_svg() {
    let fixture = include_str!("../tests/fixtures/orthogonal-skip-link.mmd");
    for direction in ["LR", "RL", "TB", "BT"] {
        let source = fixture.replacen("flowchart TB", &format!("flowchart {direction}"), 1);
        let layout = layout(&source);
        assert_orthogonal(&layout);
        assert_routes_avoid_node_interiors(&layout);
        let expected = target_center(&layout, direction, "D");
        let inputs: Vec<_> = layout
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.to == "D" && matches!(edge.from.as_str(), "B" | "C"))
            .collect();
        assert_eq!(inputs.len(), 2);
        for (_, edge) in &inputs {
            close(*edge.points.last().unwrap(), expected, direction);
            let prev = edge.points[edge.points.len() - 2];
            let inward = match direction {
                "LR" => (1.0, 0.0),
                "RL" => (-1.0, 0.0),
                "TB" => (0.0, 1.0),
                "BT" => (0.0, -1.0),
                _ => unreachable!(),
            };
            let delta = (expected.0 - prev.0, expected.1 - prev.1);
            assert!(
                delta.0 * inward.0 + delta.1 * inward.1 > 0.0,
                "{direction}: {} -> D expected {expected:?}, route {:?}",
                edge.from,
                edge.points
            );
            assert!((delta.0 * inward.1 - delta.1 * inward.0).abs() < 0.01);
        }
        for radius in [0.0, 16.0, 32.0] {
            let mut theme = Theme::modern();
            theme.node_corner_radius = Some(radius);
            let config = super::layout_config(mermaid_rs_renderer::DiagramKind::Flowchart);
            let svg = mermaid_rs_renderer::render_svg(&layout, &theme, &config);
            let document = roxmltree::Document::parse(&svg).unwrap();
            let point = |text: &str| {
                let (x, y) = text.split_once(',').unwrap();
                (x.parse::<f32>().unwrap(), y.parse::<f32>().unwrap())
            };
            let mut boundary = expected;
            if radius > 0.0 {
                let outline = document
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("path")
                            && node.attribute("data-edge-id").is_none()
                            && node.attribute("d").is_some_and(|d| d.contains(" Q "))
                    })
                    .expect("rounded diamond outline");
                let mut tokens = outline.attribute("d").unwrap().split_ascii_whitespace();
                let mut current = (0.0, 0.0);
                let mut found = false;
                while let Some(command) = tokens.next() {
                    match command {
                        "M" | "L" => current = point(tokens.next().unwrap()),
                        "Q" => {
                            let control = point(tokens.next().unwrap());
                            let end = point(tokens.next().unwrap());
                            if (control.0 - expected.0).abs() < 0.02
                                && (control.1 - expected.1).abs() < 0.02
                            {
                                // The symmetry axis intersects this quadratic at t=1/2.
                                boundary = (
                                    0.25 * current.0 + 0.5 * control.0 + 0.25 * end.0,
                                    0.25 * current.1 + 0.5 * control.1 + 0.25 * end.1,
                                );
                                found = true;
                            }
                            current = end;
                        }
                        "Z" => {}
                        _ => panic!("unexpected diamond outline {command}"),
                    }
                }
                assert!(found, "{direction}: rounded input vertex missing");
            }
            for (index, _) in &inputs {
                let id = format!("edge-{index}");
                let path = document
                    .descendants()
                    .find(|node| node.attribute("data-edge-id") == Some(id.as_str()))
                    .unwrap();
                let head = path
                    .next_sibling_element()
                    .expect("each incoming edge retains its head");
                let transform = head.attribute("transform").unwrap();
                let position = transform
                    .strip_prefix("translate(")
                    .unwrap()
                    .split_once(')')
                    .unwrap()
                    .0;
                let mut values = position
                    .split_ascii_whitespace()
                    .map(|n| n.parse::<f32>().unwrap());
                let tip = (values.next().unwrap(), values.next().unwrap());
                assert!(
                    (tip.0 - boundary.0).abs() < 0.02 && (tip.1 - boundary.1).abs() < 0.02,
                    "{direction}, radius {radius}: tip {tip:?} misses rounded vertex {boundary:?}"
                );
            }
        }
    }
}

#[test]
fn diamond_fan_in_preserves_custom_parallel_cycle_and_subgraph_ports() {
    let base = "flowchart TB\n B-->D{Decision}\n C-->D\n D-->E\n";
    for source in [
        format!("{base} B-->D\n"),
        format!("{base} D-->B\n"),
        format!("{base} linkStyle 0 stroke:#ff0000,stroke-width:4px\n"),
        format!("{base} subgraph Inputs\n B\n end\n"),
        "flowchart LR\n subgraph Inputs\n direction TB\n B-->D{Decision}\n C-->D\n D-->E\n end\n"
            .into(),
    ] {
        let layout = layout(&source);
        assert_orthogonal(&layout);
        assert_routes_avoid_node_interiors(&layout);
        let inputs: Vec<_> = layout
            .edges
            .iter()
            .filter(|edge| edge.to == "D")
            .map(|edge| *edge.points.last().unwrap())
            .collect();
        assert!(
            inputs.iter().skip(1).any(|point| {
                (point.0 - inputs[0].0).abs() > 0.01 || (point.1 - inputs[0].1).abs() > 0.01
            }),
            "special input lanes collapsed: {source}, {inputs:?}"
        );
    }
}

#[test]
fn diamond_inputs_after_refinement_budget_are_centered_at_initial_assignment() {
    // Ordinary refinement stops at 320 edges; late inputs must already be
    // centered, including in the large graph's bounded side-search tier.
    let mut source = String::from("flowchart LR\n");
    for i in 0..321 {
        source.push_str(&format!(" N{i}-->N{}\n", i + 1));
    }
    source.push_str(" B-->D{Decision}\n C-->D\n D-->E\n");
    let layout = layout(&source);
    assert_eq!(layout.edges.len(), 324);
    assert_orthogonal(&layout);
    let expected = target_center(&layout, "LR", "D");
    for edge in layout.edges.iter().filter(|edge| edge.to == "D") {
        close(*edge.points.last().unwrap(), expected, "late input");
        let prev = edge.points[edge.points.len() - 2];
        assert!(prev.0 < expected.0 && (prev.1 - expected.1).abs() < 0.001);
    }
}

#[test]
fn bounded_large_graph_side_search_keeps_orthogonal_routes() {
    // 66 edges select the existing bounded/fast side-search tier (>64).
    let mut source = String::from("flowchart LR\n");
    for i in 0..22 {
        source.push_str(&format!(
            " A{i} --> B{i} --> A{}\n A{i} --> A{}\n",
            i + 1,
            i + 1
        ));
    }
    let layout = layout(&source);
    assert_eq!(layout.edges.len(), 66);
    assert_orthogonal(&layout);
    assert_routes_avoid_node_interiors(&layout);
}

#[test]
fn compound_routes_and_rounded_svg_keep_orthogonal_lines_and_small_corner_arcs() {
    let fixture = include_str!(
        "../../../vendor/mermaid-rs-renderer-0.3.1/tests/fixtures/flowchart/ports_arrow_pathing_regression.mmd"
    );
    for direction in ["LR", "RL", "TB", "BT"] {
        let source = fixture.replacen("flowchart LR", &format!("flowchart {direction}"), 1);
        let layout = layout(&source);
        assert_orthogonal(&layout);
        assert_routes_avoid_node_interiors(&layout);
        for radius in [0.0, 16.0, 32.0] {
            let mut theme = Theme::modern();
            theme.node_corner_radius = Some(radius);
            let config = super::layout_config(mermaid_rs_renderer::DiagramKind::Flowchart);
            let svg = mermaid_rs_renderer::render_svg(&layout, &theme, &config);
            let document = roxmltree::Document::parse(&svg).unwrap();
            let mut curves = 0;
            let mut paths = 0;
            for path in document.descendants().filter(|node| {
                node.has_tag_name("path")
                    && node.attribute("data-edge-id").is_some()
                    && node.attribute("fill") == Some("none")
            }) {
                paths += 1;
                let mut tokens = path.attribute("d").unwrap().split_ascii_whitespace();
                let point = |text: &str| {
                    let (x, y) = text.split_once(',').unwrap();
                    (x.parse::<f32>().unwrap(), y.parse::<f32>().unwrap())
                };
                let aligned = |a: (f32, f32), b: (f32, f32)| {
                    (a.0 - b.0).abs() <= 0.02 || (a.1 - b.1).abs() <= 0.02
                };
                let mut current = (0.0, 0.0);
                while let Some(command) = tokens.next() {
                    match command {
                        "M" => current = point(tokens.next().unwrap()),
                        "L" => {
                            let next = point(tokens.next().unwrap());
                            assert!(
                                aligned(current, next),
                                "{direction}, radius {radius}: diagonal SVG line {}",
                                path.attribute("d").unwrap()
                            );
                            current = next;
                        }
                        "Q" => {
                            let corner = point(tokens.next().unwrap());
                            let next = point(tokens.next().unwrap());
                            assert!(aligned(current, corner) && aligned(corner, next));
                            assert!((current.0 - corner.0).hypot(current.1 - corner.1) <= 10.02);
                            assert!((next.0 - corner.0).hypot(next.1 - corner.1) <= 10.02);
                            current = next;
                            curves += 1;
                        }
                        _ => panic!("unexpected flowchart path command {command}"),
                    }
                }
            }
            assert_eq!(paths, layout.edges.len());
            assert!(
                curves > 0,
                "orthogonal bends retain the ready rounded-corner helper"
            );
        }
    }
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
        assert_orthogonal(&layout);
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
        assert_orthogonal(&layout);
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
            assert_orthogonal(&layout);
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

#[test]
fn coffee_cycle_uses_face_centers_including_the_backward_link_in_all_directions() {
    for direction in ["LR", "RL", "TB", "BT"] {
        // The reported graph includes a decision with a backward edge, which
        // deliberately bypasses the decision fan-out optimization.
        let layout = layout(&format!(
            "flowchart {direction}\n A[\"☕ Кофе\"] --> B[\"💡 Идея\"]\n B --> C[\"⌨️ Код\"]\n C --> D{{\"Работает?\"}}\n D -- Нет --> A\n D -- Да --> E[\"🚀 Релиз\"]"
        ));
        assert_eq!(layout.edges.len(), 5);
        assert_orthogonal(&layout);
        assert_routes_avoid_node_interiors(&layout);
        let mut labels = Vec::new();
        for edge in &layout.edges {
            let last = edge.points.len() - 1;
            for (id, endpoint, neighbor) in [
                (&edge.from, edge.points[0], edge.points[1]),
                (&edge.to, edge.points[last], edge.points[last - 1]),
            ] {
                let node = &layout.nodes[id];
                let center = (node.x + node.width / 2.0, node.y + node.height / 2.0);
                let centers = [
                    (node.x, center.1),
                    (node.x + node.width, center.1),
                    (center.0, node.y),
                    (center.0, node.y + node.height),
                ];
                assert!(
                    centers.iter().any(|expected| {
                        (endpoint.0 - expected.0).abs() <= EPS
                            && (endpoint.1 - expected.1).abs() <= EPS
                    }),
                    "{direction}: {} -> {}: {id} endpoint {endpoint:?} is off-center: {:?}",
                    edge.from,
                    edge.to,
                    edge.points
                );
                let outward = (endpoint.0 - center.0, endpoint.1 - center.1);
                let tangent = (neighbor.0 - endpoint.0, neighbor.1 - endpoint.1);
                assert!(
                    outward.0 * tangent.0 + outward.1 * tangent.1 > 0.0,
                    "{direction}: reversed endpoint normal at {id}: {:?}",
                    edge.points
                );
                assert!(
                    (outward.0 * tangent.1 - outward.1 * tangent.0).abs() <= EPS,
                    "{direction}: diagonal endpoint normal at {id}: {:?}",
                    edge.points
                );
            }
            if let Some(label) = &edge.label {
                let (x, y) = edge.label_anchor.expect("label remains positioned");
                labels.extend(label.lines.iter().cloned());
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
        labels.sort();
        assert_eq!(labels, ["Да", "Нет"]);
        let dimensions =
            mermaid_rs_renderer::measure_svg_dimensions(&layout, &LayoutConfig::default(), None);
        assert!(dimensions.width.is_finite() && dimensions.height.is_finite());
        for edge in &layout.edges {
            for &(x, y) in &edge.points {
                assert!(
                    x >= dimensions.viewbox_x - EPS
                        && y >= dimensions.viewbox_y - EPS
                        && x <= dimensions.viewbox_x + dimensions.viewbox_width + EPS
                        && y <= dimensions.viewbox_y + dimensions.viewbox_height + EPS,
                    "{direction}: route escaped SVG bounds: {x}, {y} in {}x{}",
                    dimensions.width,
                    dimensions.height
                );
            }
        }
    }
}
