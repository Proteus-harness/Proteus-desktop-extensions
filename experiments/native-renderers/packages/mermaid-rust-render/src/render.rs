use anyhow::{Context, Result, ensure};
use mermaid_rs_renderer::{LayoutConfig, Theme, compute_layout, parse_mermaid_strict};
use proteus_extension::ui;
use serde_json::Value;

pub(super) const MAX_SOURCE_BYTES: usize = 50_000;
const MAX_SVG_BYTES: usize = 1024 * 1024;
const MAX_GRAPH_ITEMS: usize = 500;
const MAX_DIMENSION: f64 = 4096.0;

/// The library owns parsing, graph layout, text measurement and SVG generation.
pub(super) fn render(source: &str, palette: &Value) -> Result<Value> {
    ensure!(
        source.len() <= MAX_SOURCE_BYTES,
        "Mermaid source exceeds 50,000 bytes"
    );
    let mut theme = theme(palette)?;
    let parsed = parse_mermaid_strict(source).context("unsupported or invalid Mermaid")?;
    ensure!(
        parsed.graph.nodes.len() <= MAX_GRAPH_ITEMS,
        "Mermaid exceeds 500 nodes"
    );
    ensure!(
        parsed.graph.edges.len() <= MAX_GRAPH_ITEMS,
        "Mermaid exceeds 500 edges"
    );
    // Keep potentially expensive experimental layout behind a small graph limit.
    let config = LayoutConfig::default();
    let layout = compute_layout(&parsed.graph, &theme, &config);
    let dimensions = mermaid_rs_renderer::measure_svg_dimensions(&layout, &config, None);
    let (width, height) = fitted_size(dimensions.width as f64, dimensions.height as f64)?;
    // Radius comes from logical client pixels; compensate the native SVG fit.
    // The library keeps its original viewBox and bounds corners to node sides.
    let scale =
        (width / dimensions.viewbox_width as f64).min(height / dimensions.viewbox_height as f64);
    ensure!(
        scale.is_finite() && scale > 0.0,
        "invalid Mermaid SVG scale"
    );
    theme.node_corner_radius = theme
        .node_corner_radius
        .map(|radius| (radius as f64 / scale) as f32);
    let svg = mermaid_rs_renderer::render::render_svg_with_dimensions(
        &layout,
        &theme,
        &config,
        Some((width as f32, height as f32)),
    );
    checked_svg(svg)
}

fn fitted_size(width: f64, height: f64) -> Result<(f64, f64)> {
    ensure!(
        width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
        "Mermaid returned invalid SVG dimensions"
    );
    let scale = (640.0 / width).min(MAX_DIMENSION / height).min(1.0);
    // The host accepts logical widget dimensions in 1..=4096. An extreme
    // aspect ratio can make one fitted dimension subpixel; leave at least a
    // one-pixel viewport. The library retains the natural SVG viewBox, whose
    // default preserveAspectRatio keeps the diagram itself proportional.
    Ok(((width * scale).max(1.0), (height * scale).max(1.0)))
}

fn checked_svg(svg: String) -> Result<Value> {
    ensure!(svg.len() <= MAX_SVG_BYTES, "Mermaid SVG exceeds 1 MiB");
    let document = roxmltree::Document::parse(&svg).context("Mermaid returned invalid XML")?;
    let root = document.root_element();
    ensure!(
        root.tag_name().name() == "svg",
        "Mermaid did not return an SVG root"
    );
    let width: f64 = root
        .attribute("width")
        .context("SVG width missing")?
        .parse()?;
    let height: f64 = root
        .attribute("height")
        .context("SVG height missing")?
        .parse()?;
    ensure!(
        width.is_finite()
            && height.is_finite()
            && width >= 1.0
            && height >= 1.0
            && width <= MAX_DIMENSION
            && height <= MAX_DIMENSION,
        "SVG dimensions exceed native limits"
    );
    for node in document.descendants().filter(|node| node.is_element()) {
        ensure!(
            !matches!(
                node.tag_name().name(),
                "foreignObject" | "script" | "image" | "a"
            ),
            "Mermaid SVG contains unsupported external or interactive content"
        );
        for attribute in node.attributes() {
            ensure!(
                !attribute.name().to_ascii_lowercase().starts_with("on"),
                "SVG event handlers unsupported"
            );
            if attribute.name() == "href" {
                ensure!(
                    attribute.value().starts_with('#'),
                    "external SVG references unsupported"
                );
            }
        }
    }
    drop(document);
    Ok(ui::svg(svg, width, height))
}

fn color(palette: &Value, name: &str) -> Result<String> {
    let value = palette[name]
        .as_str()
        .with_context(|| format!("theme.{name} is required"))?;
    ensure!(
        matches!(value.len(), 7 | 9)
            && value.starts_with('#')
            && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
        "theme.{name} must be #rrggbb or #rrggbbaa"
    );
    Ok(value.to_owned())
}

fn theme(palette: &Value) -> Result<Theme> {
    let foreground = color(palette, "foreground")?;
    let background = color(palette, "background")?;
    let muted = color(palette, "muted")?;
    let border = color(palette, "border")?;
    let accent = color(palette, "accent")?;
    let danger = color(palette, "danger")?;
    let surface = color(palette, "surface")?;
    let surface_alt = color(palette, "surface_alt")?;
    let family = palette["font_family"].as_str().unwrap_or("sans-serif");
    ensure!(
        !family.is_empty()
            && family.len() <= 200
            && family
                .chars()
                .all(|ch| ch.is_alphanumeric() || " _.,'\"-".contains(ch)),
        "invalid theme.font_family"
    );
    let mut theme = Theme::modern();
    let radius = palette.get("radius_lg").map_or(Ok(16.0), |value| {
        value.as_f64().context("theme.radius_lg must be a number")
    })?;
    ensure!(
        radius.is_finite() && (0.0..=256.0).contains(&radius),
        "theme.radius_lg must be in 0..=256"
    );
    theme.node_corner_radius = Some(radius as f32);
    theme.font_family = family.to_owned();
    // The native fence owns its surface. Keep the SVG canvas transparent and
    // distinguish nodes with the application's recessed background token.
    theme.primary_color = background.clone();
    theme.primary_text_color = foreground.clone();
    theme.primary_border_color = border.clone();
    theme.line_color = muted.clone();
    theme.secondary_color = surface_alt.clone();
    theme.tertiary_color = background.clone();
    theme.edge_label_background = surface.clone();
    theme.cluster_background = surface_alt.clone();
    theme.cluster_border = border.clone();
    theme.background = "none".into();
    theme.sequence_actor_fill = background.clone();
    theme.sequence_actor_border = border.clone();
    theme.sequence_actor_line = muted.clone();
    theme.sequence_note_fill = surface_alt.clone();
    theme.sequence_note_border = accent.clone();
    theme.sequence_activation_fill = surface_alt.clone();
    theme.sequence_activation_border = border.clone();
    theme.text_color = foreground.clone();
    let series = [accent, muted, danger, border.clone()];
    theme.git_colors = std::array::from_fn(|i| series[i % series.len()].clone());
    theme.git_inv_colors = std::array::from_fn(|_| background.clone());
    theme.git_branch_label_colors = std::array::from_fn(|_| foreground.clone());
    theme.git_commit_label_color = foreground.clone();
    theme.git_commit_label_background = surface.clone();
    theme.git_tag_label_color = foreground.clone();
    theme.git_tag_label_background = surface;
    theme.git_tag_label_border = border.clone();
    theme.pie_colors = std::array::from_fn(|i| series[i % series.len()].clone());
    theme.pie_title_text_color = foreground.clone();
    theme.pie_section_text_color = foreground.clone();
    theme.pie_legend_text_color = foreground;
    theme.pie_stroke_color = border.clone();
    theme.pie_outer_stroke_color = border;
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn palette() -> Value {
        json!({"foreground":"#abcdef", "background":"#111213", "muted":"#8a8b8c",
            "border":"#414243", "accent":"#2789fa", "danger":"#ee5566",
            "surface":"#222324", "surface_alt":"#303132", "font_family":"DejaVu Sans"})
    }

    #[test]
    fn corpus_uses_native_svg_and_preserves_labels_and_real_theme() {
        for (source, label) in [
            (include_str!("../tests/fixtures/flowchart.mmd"), "Запрос"),
            (include_str!("../tests/fixtures/sequence.mmd"), "Привет"),
            (include_str!("../tests/fixtures/class.mmd"), "Animal"),
        ] {
            let root = render(source, &palette()).unwrap();
            assert_eq!(root["type"], "svg");
            assert!(root["width"].as_f64().unwrap() <= 640.0);
            assert!(root["height"].as_f64().unwrap() <= MAX_DIMENSION);
            let svg = root["source"].as_str().unwrap();
            assert!(svg.contains(label), "missing label {label}");
            assert!(
                svg.contains("#abcdef"),
                "foreground did not follow host theme"
            );
            assert!(!svg.contains("foreignObject"));
        }
    }

    #[test]
    fn source_styles_are_rendered_by_the_library() {
        let root = render(
            "flowchart LR\n A[Styled] --> B[Plain]\n style A fill:#123456,stroke:#fedcba",
            &palette(),
        )
        .unwrap();
        assert!(root["source"].as_str().unwrap().contains("#123456"));
    }

    #[test]
    fn theme_is_required_validated_and_part_of_output() {
        assert!(render("flowchart LR; A-->B", &Value::Null).is_err());
        let mut colors = palette();
        colors["accent"] = json!("red\"/><script>");
        assert!(render("flowchart LR; A-->B", &colors).is_err());
        let first = render("flowchart LR; A-->B", &palette()).unwrap();
        let mut light = palette();
        light["foreground"] = json!("#123456");
        assert_ne!(first, render("flowchart LR; A-->B", &light).unwrap());
    }

    #[test]
    fn malformed_unsupported_and_oversized_input_returns_errors() {
        for source in [
            "",
            "unrecognizedDiagram\n A-->B",
            "flowchart TD\n subgraph unfinished\n A-->B",
            "flowchart TD\n -->B",
            &"x".repeat(MAX_SOURCE_BYTES + 1),
        ] {
            assert!(
                render(source, &palette()).is_err(),
                "accepted malformed input: {source:?}"
            );
        }
        let source = format!(
            "flowchart LR\n{}",
            (0..502)
                .map(|i| format!("N{i}-->N{}\n", i + 1))
                .collect::<String>()
        );
        assert!(render(&source, &palette()).is_err());
    }

    #[test]
    fn records_upstream_permissive_missing_bracket_behavior() {
        // The upstream strict validator covers selected diagnostics only; its
        // flowchart parser accepts this token as a literal node identifier.
        // Record the compatibility gap instead of writing another Mermaid parser.
        let source = include_str!("../tests/fixtures/permissive-missing-bracket.mmd");
        let parsed = parse_mermaid_strict(source).unwrap();
        assert!(parsed.graph.nodes.contains_key("A[unfinished"));
        let root = render(source, &palette()).unwrap();
        assert_eq!(root["type"], "svg");
        assert!(root["source"].as_str().unwrap().contains("A[unfinished"));
    }

    #[test]
    fn svg_bounds_and_external_content_are_checked() {
        for svg in [
            "<svg width='NaN' height='10'/>",
            "<svg width='0.5' height='10'/>",
            "<svg width='10' height='0.5'/>",
            "<svg width='10' height='4097'/>",
            "<svg width='10' height='10'><foreignObject/></svg>",
            "<svg width='10' height='10'><image href='https://example.com/x'/></svg>",
            "<svg width='10' height='10'><script/></svg>",
        ] {
            assert!(checked_svg(svg.to_owned()).is_err());
        }
        assert!(checked_svg("x".repeat(MAX_SVG_BYTES + 1)).is_err());
        let (width, height) = fitted_size(100.0, 20_000.0).unwrap();
        assert!(width < 640.0 && height <= MAX_DIMENSION);
        assert!((width / height - 100.0 / 20_000.0).abs() < 1e-8);
    }

    #[test]
    fn extreme_aspect_ratios_and_long_labels_fit_the_native_widget_contract() {
        assert_eq!(fitted_size(1_000_000.0, 1.0).unwrap(), (640.0, 1.0));
        assert_eq!(fitted_size(1.0, 1_000_000.0).unwrap(), (1.0, 4096.0));
        let label = "W".repeat(20_000);
        let root = render(&format!("flowchart LR\n A[{label}]"), &palette()).unwrap();
        for dimension in ["width", "height"] {
            assert!((1.0..=MAX_DIMENSION).contains(&root[dimension].as_f64().unwrap()));
        }
        let document = roxmltree::Document::parse(root["source"].as_str().unwrap()).unwrap();
        let viewbox: Vec<f64> = document
            .root_element()
            .attribute("viewBox")
            .unwrap()
            .split_ascii_whitespace()
            .map(|part| part.parse().unwrap())
            .collect();
        assert!(viewbox[2] / viewbox[3] > 640.0);
        assert_eq!(root["height"], 1.0);
        assert!(root["source"].as_str().unwrap().contains(&label));
    }

    #[test]
    fn app_radius_rounds_filled_diamond_and_rectangles_with_zero_as_sharp() {
        let source = "flowchart LR\n A[Request]-->B{Decision}\n B-->C(Response)";
        let mut previous_diamond = None;
        for radius in [0.0, 16.0, 32.0] {
            let mut colors = palette();
            colors["radius_lg"] = json!(radius);
            let root = render(source, &colors).unwrap();
            let document = roxmltree::Document::parse(root["source"].as_str().unwrap()).unwrap();
            assert!(document.root_element().children().any(|node| {
                node.has_tag_name("rect") && node.attribute("fill") == Some("none")
            }));
            let rects: Vec<_> = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("rect") && node.attribute("fill") == Some("#111213")
                })
                .collect();
            assert_eq!(rects.len(), 2);
            for rect in rects {
                let rx: f64 = rect.attribute("rx").unwrap().parse().unwrap();
                assert_eq!(rx == 0.0, radius == 0.0);
                assert_eq!(rect.attribute("rx"), rect.attribute("ry"));
            }
            let diamonds: Vec<_> = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("path")
                        && node.attribute("fill") == Some("#111213")
                        && node.attribute("d").is_some_and(|d| d.ends_with(" Z"))
                })
                .collect();
            if radius == 0.0 {
                assert!(diamonds.is_empty());
                assert!(
                    document
                        .descendants()
                        .any(|node| node.has_tag_name("polygon")
                            && node.attribute("fill") == Some("#111213"))
                );
            } else {
                assert_eq!(diamonds.len(), 1);
                let path = diamonds[0].attribute("d").unwrap();
                assert_eq!(path.matches(" Q ").count(), 4);
                assert!(diamonds[0].attribute("stroke").is_some());
                // All four corners have the same trim, even when the requested
                // radius is clamped by the midpoint seam's half-edge length.
                let tokens: Vec<_> = path.split_ascii_whitespace().collect();
                let point = |value: &str| {
                    let (x, y) = value.split_once(',').unwrap();
                    (x.parse::<f64>().unwrap(), y.parse::<f64>().unwrap())
                };
                let mut trim: Option<f64> = None;
                for index in tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(i, token)| (*token == "Q").then_some(i))
                {
                    let corner = point(tokens[index + 1]);
                    for endpoint in [point(tokens[index - 1]), point(tokens[index + 2])] {
                        let distance = (endpoint.0 - corner.0).hypot(endpoint.1 - corner.1);
                        if let Some(previous) = trim {
                            assert!((distance - previous).abs() < 0.02);
                        }
                        trim = Some(distance);
                    }
                }
                assert_ne!(previous_diamond.as_deref(), Some(path));
                previous_diamond = Some(path.to_owned());
            }
        }
    }

    #[test]
    fn optional_library_radius_keeps_layout_routing_and_original_defaults() {
        let source = "flowchart LR\n A[Request]-->B{Decision}\n B-->C(Response)";
        let parsed = parse_mermaid_strict(source).unwrap();
        let config = LayoutConfig::default();
        let old_theme = Theme::modern();
        assert!(old_theme.node_corner_radius.is_none());
        let original = compute_layout(&parsed.graph, &old_theme, &config);
        let old_svg = mermaid_rs_renderer::render_svg(&original, &old_theme, &config);
        let old_document = roxmltree::Document::parse(&old_svg).unwrap();
        let old_radii: Vec<_> = old_document
            .descendants()
            .filter(|node| node.has_tag_name("rect"))
            .filter_map(|node| node.attribute("rx"))
            .collect();
        assert!(old_radii.contains(&"3.00") && old_radii.contains(&"10.00"));
        for radius in [0.0, 16.0, 32.0] {
            let mut changed = old_theme.clone();
            changed.node_corner_radius = Some(radius);
            let layout = compute_layout(&parsed.graph, &changed, &config);
            assert_eq!(
                (layout.width, layout.height),
                (original.width, original.height)
            );
            for (id, node) in &layout.nodes {
                let before = &original.nodes[id];
                assert_eq!(
                    (node.x, node.y, node.width, node.height),
                    (before.x, before.y, before.width, before.height)
                );
            }
            assert_eq!(layout.edges.len(), original.edges.len());
            for (before, after) in original.edges.iter().zip(&layout.edges) {
                assert_eq!(before.points, after.points);
            }
        }
        let mut legacy_theme = serde_json::to_value(&old_theme).unwrap();
        legacy_theme
            .as_object_mut()
            .unwrap()
            .remove("node_corner_radius");
        assert!(
            serde_json::from_value::<Theme>(legacy_theme)
                .unwrap()
                .node_corner_radius
                .is_none()
        );
    }

    #[test]
    fn app_radius_is_validated_and_oversized_graph_keeps_finite_arrows() {
        for radius in [json!(-1), json!(257), json!("16"), Value::Null] {
            let mut colors = palette();
            colors["radius_lg"] = radius;
            assert!(render("flowchart LR; A-->B", &colors).is_err());
        }
        let source = format!(
            "flowchart LR\n A[{}]-->B{{Decision}}\n B-->C[Done]",
            "W".repeat(20_000)
        );
        let mut sharp: Option<Vec<((f64, f64), (f64, f64))>> = None;
        for radius in [0.0, 16.0, 32.0] {
            let mut colors = palette();
            colors["radius_lg"] = json!(radius);
            let root = render(&source, &colors).unwrap();
            let svg = root["source"].as_str().unwrap();
            assert!(!svg.contains("NaN") && !svg.contains("inf"));
            let document = roxmltree::Document::parse(svg).unwrap();
            let arrows: Vec<_> = document
                .descendants()
                .filter(|node| {
                    node.has_tag_name("path")
                        && node.attribute("data-edge-id").is_some()
                        && node.attribute("fill") == Some("none")
                })
                .filter_map(|node| node.attribute("d").map(str::to_owned))
                .collect();
            assert_eq!(arrows.len(), 2);
            let endpoints: Vec<_> = arrows
                .iter()
                .map(|path| {
                    let tokens: Vec<_> = path.split_ascii_whitespace().collect();
                    // This LR fixture has no bends; clipping a rounded boundary
                    // must not introduce them or change the other node's anchor.
                    assert_eq!(tokens.len(), 4);
                    assert_eq!((tokens[0], tokens[2]), ("M", "L"));
                    let point = |text: &str| {
                        let (x, y) = text.split_once(',').unwrap();
                        (x.parse::<f64>().unwrap(), y.parse::<f64>().unwrap())
                    };
                    (point(tokens[1]), point(tokens[3]))
                })
                .collect();
            if let Some(before) = &sharp {
                assert_eq!(before[0].0, endpoints[0].0);
                assert_eq!(before[1].1, endpoints[1].1);
                assert!(endpoints[0].1.0 > before[0].1.0);
                assert!(endpoints[1].0.0 < before[1].0.0);
                for (start, end) in &endpoints {
                    assert!(start.0 < end.0);
                    assert!((start.1 - end.1).abs() < 0.001);
                }
            } else {
                sharp = Some(endpoints);
            }
        }
    }
}
