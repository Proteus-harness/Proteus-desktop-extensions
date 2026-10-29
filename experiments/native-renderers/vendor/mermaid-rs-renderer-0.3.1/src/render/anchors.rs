//! Presentation-only endpoint clipping, including obsolete hooks inside sharp bounds.
//! Interior routing, layout and labels stay upstream.
use crate::ir::NodeShape;
use crate::layout::{EdgeLayout, NodeLayout};
use crate::theme::Theme;
use kurbo::{BezPath, Line, ParamCurve, Point, RoundedRect, Shape};
use std::borrow::Cow;
use std::collections::BTreeMap;

type RoutePoint = (f32, f32);

pub(super) fn flowchart_points<'a>(
    edge: &'a EdgeLayout,
    nodes: &BTreeMap<String, NodeLayout>,
    theme: &Theme,
) -> Cow<'a, [RoutePoint]> {
    // Use the same epsilon and retained points as the upstream path renderer.
    let mut points =
        if edge.points.windows(2).any(|pair| {
            (pair[0].0 - pair[1].0).abs() < 1e-3 && (pair[0].1 - pair[1].1).abs() < 1e-3
        }) {
            Cow::Owned(super::dedupe_points(&edge.points))
        } else {
            Cow::Borrowed(edge.points.as_slice())
        };
    let Some(radius) = theme.node_corner_radius.filter(|radius| radius.is_finite()) else {
        return points;
    };
    if points.len() < 2 {
        return points;
    }
    let last = points.len() - 1;
    // Compute both ends from the same unmodified route, including two-point edges.
    let start = nodes.get(&edge.from).and_then(|node| {
        clip_endpoint(points[0], points[1], node, radius.max(0.0))
            .map(|point| (0, point))
            .or_else(|| {
                clip_terminal_hook(points[0], points[1], *points.get(2)?, node, radius.max(0.0))
                    .map(|point| (1, point))
            })
    });
    let end = nodes.get(&edge.to).and_then(|node| {
        clip_endpoint(points[last], points[last - 1], node, radius.max(0.0))
            .map(|point| (last, point))
            .or_else(|| {
                clip_terminal_hook(
                    points[last],
                    points[last - 1],
                    *points.get(last.checked_sub(2)?)?,
                    node,
                    radius.max(0.0),
                )
                .map(|point| (last - 1, point))
            })
    });
    let (start_index, start_point) = start.unwrap_or((0, points[0]));
    let (end_index, end_point) = end.unwrap_or((last, points[last]));
    if start_index != 0 || end_index != last {
        // The replaced terminal bend can lie slightly inside the rounded shape
        // (including SVG precision). Remove it instead of doubling back from it.
        let mut clipped = Vec::with_capacity(end_index - start_index + 2);
        clipped.push(start_point);
        if start_index < end_index {
            clipped.extend_from_slice(&points[start_index + 1..end_index]);
        }
        clipped.push(end_point);
        points = Cow::Owned(clipped);
    } else {
        if start_point != points[0] {
            points.to_mut()[0] = start_point;
        }
        if end_point != points[last] {
            points.to_mut()[last] = end_point;
        }
    }
    // Clipping can make a tip coincide with a retained bend. Share this final
    // deduped route between the SVG stem and both arrowhead tangent calculations.
    if points
        .windows(2)
        .any(|pair| (pair[0].0 - pair[1].0).abs() < 1e-3 && (pair[0].1 - pair[1].1).abs() < 1e-3)
    {
        points = Cow::Owned(super::dedupe_points(&points));
    }
    points
}

// Upstream sometimes ends a route with a bend on the old sharp bounding box.
// Continue its preceding incoming segment and replace the obsolete terminal bend
// as well as the old tip. Interior routing outside that local hook stays intact.
fn clip_terminal_hook(
    endpoint: RoutePoint,
    neighbor: RoutePoint,
    previous: RoutePoint,
    node: &NodeLayout,
    radius: f32,
) -> Option<RoutePoint> {
    let inside_sharp_bounds = |point: RoutePoint| {
        point.0 >= node.x - 0.02
            && point.0 <= node.x + node.width + 0.02
            && point.1 >= node.y - 0.02
            && point.1 <= node.y + node.height + 0.02
    };
    // Only a local terminal hook inside the obsolete sharp bounds is eligible;
    // do not remove or reroute arbitrary outside/interior segments.
    if !inside_sharp_bounds(endpoint) || !inside_sharp_bounds(neighbor) {
        return None;
    }
    clip_endpoint(neighbor, previous, node, radius)
}

fn outline(node: &NodeLayout, radius: f32) -> Option<BezPath> {
    if ![node.x, node.y, node.width, node.height, radius]
        .iter()
        .all(|v| v.is_finite())
        || node.width <= 0.0
        || node.height <= 0.0
    {
        return None;
    }
    match node.shape {
        NodeShape::Rectangle | NodeShape::ActorBox | NodeShape::RoundRect => {
            // Match the precision of the actual SVG rect attributes.
            let svg_value = |value: f32| format!("{value:.2}").parse::<f64>().unwrap();
            let x = svg_value(node.x);
            let y = svg_value(node.y);
            let width = svg_value(node.width);
            let height = svg_value(node.height);
            if width <= 0.0 || height <= 0.0 {
                return None;
            }
            let radius = svg_value(radius.min(node.width / 2.0).min(node.height / 2.0));
            Some(RoundedRect::new(x, y, x + width, y + height, radius).to_path(1e-5))
        }
        NodeShape::Diamond => {
            // Reuse the exact existing quadratic SVG outline, including its trim cap.
            BezPath::from_svg(&super::rounded_diamond_path(node, radius)).ok()
        }
        _ => None,
    }
}

fn clip_endpoint(
    endpoint: RoutePoint,
    neighbor: RoutePoint,
    node: &NodeLayout,
    radius: f32,
) -> Option<RoutePoint> {
    if ![endpoint.0, endpoint.1, neighbor.0, neighbor.1]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let path = outline(node, radius)?;
    let origin = Point::new(neighbor.0 as f64, neighbor.1 as f64);
    // An inside neighbor has no incoming outside segment. Keep the route safely intact.
    if path.contains(origin) {
        return None;
    }
    let direction = Point::new(endpoint.0 as f64, endpoint.1 as f64) - origin;
    let length = direction.hypot();
    if length <= 1e-3 {
        return None;
    }
    // Rounded corners retreat behind the old sharp endpoint. Search at least one
    // entire node diagonal beyond it; take the first forward hit, never the exit.
    let reach = length + (node.width as f64).hypot(node.height as f64) + 1.0;
    let probe = Line::new(origin, origin + direction * (reach / length));
    let first_hit = path
        .segments()
        .flat_map(|segment| segment.intersect_line(probe))
        .map(|hit| hit.line_t)
        .filter(|t| t.is_finite() && *t > 1e-9 && *t <= 1.0)
        .min_by(f64::total_cmp)?;
    let hit = probe.eval(first_hit);
    Some((hit.x as f32, hit.y as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LayoutConfig;
    use crate::ir::{Direction, Graph};
    use crate::layout::compute_layout;
    use kurbo::ParamCurveNearest;

    fn fixture(shape: NodeShape) -> (EdgeLayout, BTreeMap<String, NodeLayout>) {
        let mut graph = Graph::new();
        graph.ensure_node("A", Some("A".into()), Some(shape));
        graph.ensure_node("B", Some("B".into()), Some(shape));
        graph.edges.push(crate::ir::Edge {
            from: "A".into(),
            to: "B".into(),
            label: None,
            start_label: None,
            end_label: None,
            directed: true,
            arrow_start: true,
            arrow_end: true,
            arrow_start_kind: None,
            arrow_end_kind: None,
            start_decoration: None,
            end_decoration: None,
            style: crate::ir::EdgeStyle::Solid,
        });
        let layout = compute_layout(&graph, &Theme::modern(), &LayoutConfig::default());
        (layout.edges[0].clone(), layout.nodes)
    }

    fn node(shape: NodeShape) -> NodeLayout {
        let (_, mut nodes) = fixture(shape);
        let mut node = nodes.remove("B").unwrap();
        node.x = 100.0;
        node.y = 100.0;
        node.width = 100.0;
        node.height = 80.0;
        node
    }

    fn on_outline(point: RoutePoint, node: &NodeLayout, radius: f32) {
        let point = Point::new(point.0 as f64, point.1 as f64);
        let distance = outline(node, radius)
            .unwrap()
            .segments()
            .map(|segment| segment.nearest(point, 1e-8).distance_sq)
            .fold(f64::INFINITY, f64::min)
            .sqrt();
        assert!(
            distance < 0.002,
            "endpoint {point:?} misses outline by {distance}"
        );
    }

    #[test]
    fn rectangles_touch_rounded_corners_from_four_directions_at_all_radii() {
        for shape in [
            NodeShape::Rectangle,
            NodeShape::ActorBox,
            NodeShape::RoundRect,
        ] {
            let node = node(shape);
            for radius in [0.0, 16.0, 32.0] {
                for (endpoint, neighbor) in [
                    ((100.0, 104.0), (20.0, 104.0)),
                    ((200.0, 104.0), (280.0, 104.0)),
                    ((104.0, 100.0), (104.0, 20.0)),
                    ((104.0, 180.0), (104.0, 260.0)),
                ] {
                    let hit = clip_endpoint(endpoint, neighbor, &node, radius).unwrap();
                    on_outline(hit, &node, radius);
                    if radius > 0.0 {
                        assert!((hit.0 - endpoint.0).hypot(hit.1 - endpoint.1) > 1.0);
                    } else {
                        assert_eq!(hit, endpoint);
                    }
                    let cross = (hit.0 - neighbor.0) * (endpoint.1 - neighbor.1)
                        - (hit.1 - neighbor.1) * (endpoint.0 - neighbor.0);
                    assert!(cross.abs() < 0.002, "endpoint changed segment direction");
                    // Independent analytic circle check, rather than only testing our own path.
                    if radius > 0.0 {
                        let cx = if hit.0 < 150.0 {
                            100.0 + radius
                        } else {
                            200.0 - radius
                        };
                        let cy = if hit.1 < 140.0 {
                            100.0 + radius
                        } else {
                            180.0 - radius
                        };
                        assert!(((hit.0 - cx).hypot(hit.1 - cy) - radius).abs() < 0.002);
                    }
                }
            }
        }
    }

    #[test]
    fn diamonds_use_the_rendered_quadratic_outline_in_four_directions() {
        let node = node(NodeShape::Diamond);
        for radius in [0.0, 16.0, 32.0] {
            for (endpoint, neighbor) in [
                ((100.0, 140.0), (20.0, 140.0)),
                ((200.0, 140.0), (280.0, 140.0)),
                ((150.0, 100.0), (150.0, 20.0)),
                ((150.0, 180.0), (150.0, 260.0)),
            ] {
                let hit = clip_endpoint(endpoint, neighbor, &node, radius).unwrap();
                on_outline(hit, &node, radius);
                let retreat = (hit.0 - endpoint.0).hypot(hit.1 - endpoint.1);
                if radius == 0.0 {
                    assert!(retreat < 0.002);
                } else {
                    assert!(retreat > 1.0);
                }
            }
        }
    }

    #[test]
    fn bidirectional_route_shares_deduped_endpoints_stem_and_head_tangents() {
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        let mut source = node(NodeShape::Rectangle);
        source.x = -100.0;
        nodes.insert("A".into(), source);
        nodes.insert("B".into(), node(NodeShape::Rectangle));
        edge.points = vec![
            (0.0, 104.0),
            (0.0, 104.0),
            (30.0, 104.0),
            (50.0, 80.0),
            (80.0, 104.0),
            (100.0, 104.0),
            (100.0, 104.0),
        ];
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(points.len(), 5);
        assert_eq!(&points[1..4], &edge.points[2..5]);
        on_outline(points[0], &nodes["A"], 32.0);
        on_outline(points[4], &nodes["B"], 32.0);
        assert!(points[0].0 < 0.0 && points[4].0 > 100.0);
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, true) + 180.0,
            180.0
        );
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, false),
            0.0
        );
        let path = super::super::rounded_polyline_path_deduped(&points, 10.0);
        assert!(path.starts_with(&format!("M {:.3},{:.3}", points[0].0, points[0].1)));
        assert!(path.ends_with(&format!("L {:.3},{:.3}", points[4].0, points[4].1)));
    }

    #[test]
    fn svg_stem_and_both_heads_use_corrected_diagonal_tangents() {
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        let mut source = node(NodeShape::Rectangle);
        source.x = -100.0;
        nodes.insert("A".into(), source);
        nodes.insert("B".into(), node(NodeShape::Rectangle));
        edge.points = vec![
            (0.0, 100.0),
            (0.0, 100.0),
            (20.0, 80.0),
            (80.0, 80.0),
            (100.0, 100.0),
            (100.0, 100.0),
        ];
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        let points = flowchart_points(&edge, &nodes, &theme).into_owned();
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, true) + 180.0,
            135.0
        );
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, false),
            45.0
        );
        on_outline(points[0], &nodes["A"], 32.0);
        on_outline(points[3], &nodes["B"], 32.0);
        let config = LayoutConfig::default();
        let mut layout = compute_layout(&Graph::new(), &theme, &config);
        layout.nodes = nodes;
        layout.edges = vec![edge];
        let svg = super::super::render_svg(&layout, &theme, &config);
        let path = super::super::rounded_polyline_path_deduped(&points, 10.0);
        assert!(svg.contains(&format!("d=\"{path}\"")));
        for (point, angle) in [(points[0], 135.0), (points[3], 45.0)] {
            assert!(svg.contains(&format!(
                "translate({:.2} {:.2}) rotate({angle:.2})",
                point.0, point.1
            )));
        }
        // Two-point bidirectional edges correct both ends independently.
        layout.edges[0].points = vec![(0.0, 104.0), (100.0, 104.0)];
        let points = flowchart_points(&layout.edges[0], &layout.nodes, &theme);
        on_outline(points[0], &layout.nodes["A"], 32.0);
        on_outline(points[1], &layout.nodes["B"], 32.0);
        assert!(points[0].0 < 0.0 && points[1].0 > 100.0);
    }

    #[test]
    fn two_point_and_short_segments_extend_to_the_first_entry() {
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        nodes.remove("A");
        nodes.insert("B".into(), node(NodeShape::Rectangle));
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        edge.points = vec![(99.99, 100.0), (100.0, 100.0)];
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(points[0], edge.points[0]);
        on_outline(points[1], &nodes["B"], 32.0);
        assert!(
            (points[1].0 - 132.0).abs() < 0.002,
            "picked exit instead of first entry"
        );
        assert_eq!(points[1].1, 100.0);
    }

    #[test]
    fn real_preview_branches_replace_only_obsolete_terminal_hooks() {
        // Read the permanent native fixture, both from the vendor location and
        // from the maintained private test runner under target/verification.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|root| root.join("experiments/native-renderers/fixtures/preview.md"))
            .find(|path| path.is_file())
            .expect("permanent preview.md fixture");
        let markdown = std::fs::read_to_string(fixture).unwrap();
        let source = markdown
            .split_once("```mermaid\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0;
        let parsed = crate::parse_mermaid(source).unwrap();
        let mut theme = Theme::modern();
        theme.font_family = "sans-serif".into();
        let config = LayoutConfig::default();
        let layout = compute_layout(&parsed.graph, &theme, &config);
        let dimensions = crate::measure_svg_dimensions(&layout, &config, None);
        let scale = (640.0 / dimensions.width).min(1.0);
        for logical_radius in [0.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
            let radius = logical_radius / scale;
            theme.node_corner_radius = Some(radius);
            for edge in &layout.edges {
                let points = flowchart_points(edge, &layout.nodes, &theme);
                let last = points.len() - 1;
                on_outline(points[0], &layout.nodes[&edge.from], radius);
                on_outline(points[last], &layout.nodes[&edge.to], radius);
                assert!(
                    points.windows(2).all(|pair| {
                        (pair[0].0 - pair[1].0).hypot(pair[0].1 - pair[1].1) > 1e-3
                    })
                );
                let svg = super::super::render_svg(&layout, &theme, &config);
                let path = super::super::rounded_polyline_path_deduped(&points, 10.0);
                assert!(svg.contains(&format!("d=\"{path}\"")));
                let angle = crate::edge_geometry::edge_endpoint_angle(&points, false);
                assert!(svg.contains(&format!(
                    "translate({:.2} {:.2}) rotate({angle:.2})",
                    points[last].0, points[last].1
                )));
                if edge.from == "C" && edge.to == "E" {
                    // Every setting must eliminate the former upward target hook.
                    assert_eq!(angle, 0.0, "radius {logical_radius}");
                    // The upstream fan-out backport exits the lower diamond
                    // side vertically. Rounded-outline clipping must preserve
                    // that outgoing tangent rather than the former rightward
                    // shortcut that collapsed this branch onto the main apex.
                    assert_eq!(
                        crate::edge_geometry::edge_endpoint_angle(&points, true),
                        90.0,
                        "radius {logical_radius} source fan tangent"
                    );
                    // Only obsolete terminal bends are eligible for removal.
                    for interior in &edge.points[2..edge.points.len() - 2] {
                        assert!(points.contains(interior));
                    }
                }
            }
        }
    }

    #[test]
    fn terminal_fallback_preserves_outside_hooks_and_clips_inside_bends_without_reversal() {
        let node = node(NodeShape::Rectangle);
        assert!(
            clip_terminal_hook((100.0, 104.0), (90.0, 104.0), (20.0, 104.0), &node, 32.0).is_none()
        );
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        nodes.remove("A");
        nodes.insert("B".into(), node);
        edge.points = vec![(20.0, 104.0), (150.0, 104.0), (100.0, 104.0)];
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(points.len(), 2);
        on_outline(points[1], &nodes["B"], 32.0);
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, false),
            0.0
        );
    }

    #[test]
    fn terminal_fallback_drops_a_bend_equal_to_the_new_tip() {
        let node = node(NodeShape::Rectangle);
        let endpoint = (100.0, 100.0);
        let neighbor = (132.0, 100.0);
        let previous = (132.0, 20.0);
        let hit = clip_endpoint(neighbor, previous, &node, 32.0).unwrap();
        assert!((hit.0 - neighbor.0).hypot(hit.1 - neighbor.1) < 1e-3);
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        nodes.remove("A");
        nodes.insert("B".into(), node);
        edge.points = vec![previous, neighbor, endpoint];
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(points.len(), 2);
        on_outline(points[1], &nodes["B"], 32.0);
        assert!((points[1].0 - points[0].0).hypot(points[1].1 - points[0].1) > 1e-3);
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, false),
            90.0
        );
    }

    #[test]
    fn both_terminal_trims_use_original_indices_for_short_routes() {
        let (mut edge, mut nodes) = fixture(NodeShape::Rectangle);
        let mut source = node(NodeShape::Rectangle);
        source.x = 0.0;
        let mut target = source.clone();
        let mut theme = Theme::modern();
        theme.node_corner_radius = Some(32.0);
        for (target_x, route) in [
            (
                200.0,
                vec![
                    (100.0, 104.0),
                    (100.0, 110.0),
                    (200.0, 110.0),
                    (200.0, 104.0),
                ],
            ),
            (80.0, vec![(100.0, 100.0), (90.0, 110.0), (80.0, 100.0)]),
        ] {
            target.x = target_x;
            nodes.insert("A".into(), source.clone());
            nodes.insert("B".into(), target.clone());
            edge.points = route;
            let points = flowchart_points(&edge, &nodes, &theme);
            assert_eq!(points.len(), 2, "route {:?} => {:?}", edge.points, points);
            on_outline(points[0], &nodes["A"], 32.0);
            on_outline(points[1], &nodes["B"], 32.0);
            assert!((points[1].0 - points[0].0).hypot(points[1].1 - points[0].1) > 1e-3);
        }
        edge.points = vec![(70.0, 100.0), (110.0, 100.0)];
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(points.len(), 2);
        on_outline(points[0], &nodes["A"], 32.0);
        on_outline(points[1], &nodes["B"], 32.0);
    }

    #[test]
    fn sharp_diamond_anchors_match_svg_coordinate_precision() {
        let mut node = node(NodeShape::Diamond);
        node.x = 100.004;
        node.y = 100.004;
        let hit = clip_endpoint((150.004, 100.004), (150.004, 20.0), &node, 0.0).unwrap();
        let rendered = super::super::shape_svg(&node, &Theme::modern(), &LayoutConfig::default());
        assert!(rendered.contains("150.00,100.00"));
        assert!((hit.1 - 100.0).abs() < 0.005);
        on_outline(hit, &node, 0.0);
    }

    #[test]
    fn unsafe_or_missing_intersections_preserve_endpoints() {
        let node = node(NodeShape::Rectangle);
        for (endpoint, neighbor) in [
            ((100.0, 140.0), (150.0, 140.0)), // inside neighbor
            ((100.0, 140.0), (100.0, 140.0)), // zero segment
            ((100.0, 90.0), (20.0, 90.0)),    // misses outline
            ((f32::NAN, 140.0), (20.0, 140.0)),
        ] {
            assert!(clip_endpoint(endpoint, neighbor, &node, 32.0).is_none());
        }
        let (mut edge, nodes) = fixture(NodeShape::Rectangle);
        edge.points = vec![(1.0, 2.0); 3];
        assert_eq!(
            &*flowchart_points(&edge, &nodes, &Theme::modern()),
            &[(1.0, 2.0)]
        );
    }

    #[test]
    fn default_radius_preserves_original_routes_and_fixes_duplicate_tangents() {
        let (mut edge, nodes) = fixture(NodeShape::Diamond);
        let theme = Theme::modern();
        assert!(matches!(
            flowchart_points(&edge, &nodes, &theme),
            Cow::Borrowed(_)
        ));
        edge.points = vec![(0.0, 0.0), (20.0, 20.0), (20.0, 20.0)];
        let points = flowchart_points(&edge, &nodes, &theme);
        assert_eq!(
            crate::edge_geometry::edge_endpoint_angle(&points, false),
            45.0
        );
        assert_eq!(&*points, &edge.points[..2]);
    }

    #[test]
    fn endpoints_remain_on_outline_after_svg_scale_and_fit() {
        let node = node(NodeShape::Diamond);
        let hit = clip_endpoint((150.0, 100.0), (150.0, 20.0), &node, 32.0).unwrap();
        let path = outline(&node, 32.0).unwrap();
        for scale in [0.25, 1.0, 4.0] {
            let fit = kurbo::Affine::translate((31.0, 19.0)) * kurbo::Affine::scale(scale);
            let transformed = fit * path.clone();
            let point = fit * Point::new(hit.0 as f64, hit.1 as f64);
            let distance = transformed
                .segments()
                .map(|segment| segment.nearest(point, 1e-8).distance_sq)
                .fold(f64::INFINITY, f64::min)
                .sqrt();
            assert!(distance < 0.002);
        }
        // Exercise real routed fixtures for every diagram direction as well.
        for direction in [
            Direction::TopDown,
            Direction::BottomTop,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            let mut graph = Graph::new();
            graph.direction = direction;
            graph.ensure_node("A", Some("A".into()), Some(NodeShape::Diamond));
            graph.ensure_node("B", Some("B".into()), Some(NodeShape::RoundRect));
            let (edge, _) = fixture(NodeShape::Rectangle);
            graph.edges.push(crate::ir::Edge {
                from: edge.from,
                to: edge.to,
                label: None,
                start_label: None,
                end_label: None,
                directed: true,
                arrow_start: true,
                arrow_end: true,
                arrow_start_kind: None,
                arrow_end_kind: None,
                start_decoration: None,
                end_decoration: None,
                style: crate::ir::EdgeStyle::Solid,
            });
            let mut theme = Theme::modern();
            theme.node_corner_radius = Some(32.0);
            let layout = compute_layout(&graph, &theme, &LayoutConfig::default());
            let points = flowchart_points(&layout.edges[0], &layout.nodes, &theme);
            on_outline(points[0], &layout.nodes["A"], 32.0);
            on_outline(*points.last().unwrap(), &layout.nodes["B"], 32.0);
        }
    }
}
