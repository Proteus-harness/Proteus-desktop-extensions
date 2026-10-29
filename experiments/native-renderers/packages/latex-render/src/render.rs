use anyhow::{Context, anyhow, ensure};
use latex_rust::{Color, Dim, MathFont, SvgOptions, latex_to_svg};
use proteus_extension::ui;
use serde_json::Value;

const MAX_SOURCE: usize = 32 * 1024;
const MAX_SVG: usize = 1024 * 1024;
const MAX_DIMENSION: f64 = 4096.0;

pub(super) struct Style {
    pub color: Color,
    pub opacity: f64,
    pub font_size_px: f64,
}

impl Style {
    pub fn from_context(context: &Value) -> anyhow::Result<Self> {
        let theme = context
            .get("theme")
            .ok_or_else(|| anyhow!("content.context.theme is required"))?;
        let foreground = theme
            .get("foreground")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("theme.foreground is required"))?;
        let hex = foreground.strip_prefix('#').unwrap_or("");
        ensure!(
            matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "theme.foreground must be #rrggbb or #rrggbbaa"
        );
        let channel = |offset| u8::from_str_radix(&hex[offset..offset + 2], 16);
        let color = Color::rgb(channel(0)?, channel(2)?, channel(4)?);
        let opacity = if hex.len() == 8 {
            f64::from(channel(6)?) / 255.0
        } else {
            1.0
        };
        // Protocol 4 currently sends font families but no font size. STIX has
        // the OpenType MATH metrics needed by the renderer; host body fonts do
        // not necessarily have them. Use host size when available, else 16 px.
        let font_size_px = match theme.get("font_size") {
            Some(value) => value
                .as_f64()
                .ok_or_else(|| anyhow!("theme.font_size must be numeric"))?,
            None => 16.0,
        };
        ensure!(
            font_size_px.is_finite() && (8.0..=72.0).contains(&font_size_px),
            "theme.font_size must be between 8 and 72 logical pixels"
        );
        Ok(Self {
            color,
            opacity,
            font_size_px,
        })
    }
}

pub(super) fn check_source(source: &str) -> anyhow::Result<()> {
    ensure!(!source.trim().is_empty(), "math source is empty");
    ensure!(source.len() <= MAX_SOURCE, "math source exceeds 32 KiB");
    // Additional admission budgets alongside latex-rust's own recursion limit,
    // not another math parser: syntax and compatibility remain its job.
    let mut depth: usize = 0;
    let mut recursive_markers = 0;
    for ch in source.chars() {
        match ch {
            '{' => {
                depth += 1;
                ensure!(depth <= 64, "math nesting exceeds 64");
            }
            '}' => depth = depth.saturating_sub(1),
            '\\' | '^' | '_' | '\'' => {
                // bind_scripts also nests AST rows for consecutive primes;
                // braces alone do not bound the upstream recursive layout.
                recursive_markers += 1;
                ensure!(
                    recursive_markers <= 256,
                    "math source exceeds 256 command, script or prime markers"
                );
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn render(source: &str, font: &MathFont, style: &Style) -> anyhow::Result<Value> {
    check_source(source)?;
    let mut options = SvgOptions::new();
    // SVG uses points; the native widget uses logical CSS pixels (96/72).
    options.font_size_pt = Dim::ratio((style.font_size_px * 3000.0).round() as i64, 4000);
    options.color = style.color;
    options.display = true;
    let mut svg = latex_to_svg(source, font, &options).context("LaTeX math rendering failed")?;
    ensure!(svg.len() <= MAX_SVG, "math SVG exceeds 1 MiB");
    if style.opacity < 1.0 {
        svg = svg.replacen(
            "<g fill=",
            &format!("<g opacity=\"{}\" fill=", style.opacity),
            1,
        );
    }
    let (width, height) = dimensions(&svg)?;
    ensure!(svg.len() <= MAX_SVG, "math SVG exceeds 1 MiB");
    Ok(ui::svg(svg, width, height))
}

fn dimensions(svg: &str) -> anyhow::Result<(f64, f64)> {
    let document = roxmltree::Document::parse_with_options(
        svg,
        roxmltree::ParsingOptions {
            nodes_limit: 100_000,
            ..Default::default()
        },
    )
    .context("invalid generated SVG")?;
    let root = document.root_element();
    ensure!(
        root.tag_name().name() == "svg",
        "generated document is not SVG"
    );
    let dimension = |name| -> anyhow::Result<f64> {
        let points = root
            .attribute(name)
            .and_then(|value| value.strip_suffix("pt"))
            .ok_or_else(|| anyhow!("generated SVG has no {name} in points"))?
            .parse::<f64>()?;
        let pixels = points * 96.0 / 72.0;
        ensure!(
            pixels.is_finite() && pixels > 0.0 && pixels <= MAX_DIMENSION,
            "math SVG {name} must be between 0 and 4096 pixels"
        );
        // The native widget requires dimensions >= 1 logical pixel. Preserve
        // the SVG viewBox and let tiny, valid outlines occupy a one-pixel box.
        Ok(pixels.max(1.0))
    };
    Ok((dimension("width")?, dimension("height")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn style(color: &str) -> Style {
        Style::from_context(&json!({"theme":{"foreground":color}})).unwrap()
    }

    #[test]
    fn real_math_layout_emits_self_contained_paths_with_native_sizes() {
        let font = MathFont::stix_two_math().unwrap();
        for source in [
            r"\frac{1}{2}",
            r"\sqrt{x^2+y_1}",
            r"\sum_{i=1}^{n} i^2",
            r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}",
            "α + β = γ",
        ] {
            let root = render(source, &font, &style("#ddeeff")).unwrap();
            let svg = root["source"].as_str().unwrap();
            let document = roxmltree::Document::parse(svg).unwrap();
            assert!(
                document.descendants().any(|node| node.has_tag_name("path")),
                "{source}"
            );
            for tag in ["text", "script", "image", "foreignObject", "style"] {
                assert!(
                    !document.descendants().any(|node| node.has_tag_name(tag)),
                    "{source}: {tag}"
                );
            }
            assert!(svg.contains("#ddeeff"));
            assert!(root["width"].as_f64().unwrap() > 0.0);
            assert!(root["height"].as_f64().unwrap() > 0.0);
        }
    }

    #[test]
    fn theme_alpha_and_point_to_pixel_conversion_are_preserved() {
        let font = MathFont::stix_two_math().unwrap();
        let mut small = style("#12345680");
        let small_root = render("x", &font, &small).unwrap();
        small.font_size_px = 32.0;
        let big_root = render("x", &font, &small).unwrap();
        let ratio = big_root["width"].as_f64().unwrap() / small_root["width"].as_f64().unwrap();
        assert!((ratio - 2.0).abs() < 0.001);
        assert!(
            small_root["source"]
                .as_str()
                .unwrap()
                .contains("opacity=\"0.501960")
        );
        assert_eq!(
            dimensions(r#"<svg width="72pt" height="36pt"/>"#).unwrap(),
            (96.0, 48.0)
        );
    }

    #[test]
    fn cyrillic_text_in_user_mood_fraction_has_real_glyph_outlines() {
        let font = MathFont::stix_two_math().unwrap();
        let source = "\\text{Настроение} =\n\\frac{\\text{кофе} + \\text{интересные задачи}}\n{1 + \\text{количество багов}}\n";
        // Prove the embedded font covers the text instead of silently drawing
        // .notdef placeholders or depending on a machine's installed fonts.
        let text = "Настроениекофеинтересныезадачиколичествобагов";
        for ch in text.chars() {
            assert_ne!(font.glyph(ch).unwrap().glyph_id, 0, "{ch}");
        }
        let root = render(source, &font, &style("#ddeeff")).unwrap();
        let document = roxmltree::Document::parse(root["source"].as_str().unwrap()).unwrap();
        let paths = document
            .descendants()
            .filter(|node| node.has_tag_name("path"))
            .count();
        assert!(
            paths >= text.chars().count(),
            "{paths} outlines for {} text glyphs",
            text.chars().count()
        );
        assert!(root["width"].as_f64().unwrap() > 100.0);
        assert!(root["height"].as_f64().unwrap() > 20.0);
    }

    #[test]
    fn integral_limits_and_nested_powers_use_script_glyph_scales() {
        let font = MathFont::stix_two_math().unwrap();
        let params = latex_rust::MathParams::from_font(&font).unwrap();
        let base = 12.0 / f64::from(font.units_per_em());
        let script = base * f64::from(params.script_percent_scale_down) / 100.0;
        let scriptscript = base * f64::from(params.script_script_percent_scale_down) / 100.0;
        for source in [r"\int_0^\infty x\,dx", "x^2", "x_1", "x^{y^2}"] {
            let root = render(source, &font, &style("#ffffff")).unwrap();
            let document = roxmltree::Document::parse(root["source"].as_str().unwrap()).unwrap();
            let scales = document
                .descendants()
                .filter(|node| node.has_tag_name("path"))
                .map(|node| {
                    node.attribute("transform")
                        .unwrap()
                        .split("scale(")
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let has_scale =
                |expected: f64| scales.iter().any(|scale| (scale - expected).abs() < 1e-10);
            assert!(
                has_scale(base),
                "missing body scale in {source}: {scales:?}"
            );
            assert!(
                has_scale(script),
                "missing script scale in {source}: {scales:?}"
            );
            if source == "x^{y^2}" {
                assert!(
                    has_scale(scriptscript),
                    "missing nested script scale: {scales:?}"
                );
            }
        }
    }

    #[test]
    fn unsupported_commands_and_invalid_theme_are_errors() {
        let font = MathFont::stix_two_math().unwrap();
        assert!(render(r"\thiscommanddoesnotexist{x}", &font, &style("#ffffff")).is_err());
        assert!(render(r"\frac{1}{", &font, &style("#ffffff")).is_err());
        assert!(Style::from_context(&json!({})).is_err());
        assert!(Style::from_context(&json!({"theme":{"foreground":"red"}})).is_err());
    }

    #[test]
    fn source_and_output_admission_are_bounded() {
        assert!(check_source(" ").is_err());
        assert!(check_source(&"x".repeat(MAX_SOURCE + 1)).is_err());
        assert!(check_source(&"{".repeat(65)).is_err());
        assert!(check_source(&r"\alpha ".repeat(257)).is_err());
        for svg in [
            r#"<svg width="5000pt" height="1pt"/>"#,
            r#"<svg width="NaNpt" height="1pt"/>"#,
            r#"<svg width="0pt" height="1pt"/>"#,
        ] {
            assert!(dimensions(svg).is_err());
        }
        let font = MathFont::stix_two_math().unwrap();
        assert!(render(&"x".repeat(1000), &font, &style("#ffffff")).is_err());
    }

    #[test]
    fn unbraced_prime_and_script_chains_are_rejected_before_upstream_parsing() {
        let font = MathFont::stix_two_math().unwrap();
        let primes = format!("x{}", "'".repeat(20_000));
        assert!(
            render(&primes, &font, &style("#ffffff"))
                .unwrap_err()
                .to_string()
                .contains("256 command, script or prime markers")
        );
        for source in [
            format!("x{}", "^x".repeat(257)),
            format!("x{}", "_x".repeat(257)),
            format!(r"\alpha{}", "'".repeat(256)),
        ] {
            assert!(check_source(&source).is_err());
        }
        assert!(check_source(&format!("x{}", "'".repeat(256))).is_ok());
    }

    #[test]
    fn subpixel_dimensions_still_produce_valid_native_widgets() {
        assert_eq!(
            dimensions(r#"<svg width="0.1pt" height="0.2pt"/>"#).unwrap(),
            (1.0, 1.0)
        );
        let font = MathFont::stix_two_math().unwrap();
        let mut small = style("#ffffff");
        small.font_size_px = 8.0;
        let root = render(".", &font, &small).unwrap();
        assert!(root["width"].as_f64().unwrap() >= 1.0);
        assert!(root["height"].as_f64().unwrap() >= 1.0);
    }
}
