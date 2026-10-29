// Geometry checks adapted from latex-rust 2.1.1 tests/amsmath_grid.rs and
// tests/integral_scripts.rs (MIT OR Apache-2.0), Copyright 2026 Jeffrey S Carr.
// These pin upstream behavior needed by our fixtures; no local layout engine.
use latex_rust::{
    BoxContent, Dim, MathBox, MathFont, MathParams, MathStyle, layout_with_em_size_pt, parse,
};

fn layout(source: &str, font: &MathFont) -> MathBox {
    layout_with_em_size_pt(
        &parse(source).unwrap(),
        font,
        MathStyle::Display,
        &Dim::from_i64(12),
    )
    .unwrap()
}

fn children(tree: &MathBox) -> &[MathBox] {
    let BoxContent::HList(children) = &tree.content else {
        panic!("expected horizontal layout")
    };
    children
}

#[test]
fn preview_matrices_and_delimiters_share_the_math_axis() {
    let font = MathFont::stix_two_math().unwrap();
    let params = MathParams::from_font(&font).unwrap();
    let axis = &params.axis_height * &params.scale(MathStyle::Display);
    for source in [
        r"\begin{pmatrix}a&b\\c&d\end{pmatrix}",
        r"\begin{pmatrix}x\\y\end{pmatrix}",
        r"\begin{pmatrix}ax+by\\cx+dy\end{pmatrix}",
    ] {
        let tree = layout(source, &font);
        let outer = children(&tree);
        assert_eq!(outer.len(), 3, "left delimiter, matrix, right delimiter");
        assert!(matches!(outer[1].content, BoxContent::VList(_)));
        for part in outer {
            let height = (&part.height + &part.shift).clamp_nonneg();
            let depth = (&part.depth - &part.shift).clamp_nonneg();
            let center = &(&height - &depth) / &Dim::from_i64(2);
            assert!(
                center.eq_dim(&axis),
                "matrix/delimiter center differs from MATH axis: {source}"
            );
        }
    }
}

#[test]
fn display_integral_limits_follow_operator_extents_and_italic_offset() {
    let font = MathFont::stix_two_math().unwrap();
    let params = MathParams::from_font(&font).unwrap();
    let tree = layout(r"\int_0^\infty", &font);
    let outer = children(&tree);
    let (base, italic_kern, slot) = (&outer[0], &outer[1], &outer[2]);
    assert!(matches!(base.content, BoxContent::Glyph { ch: '∫', .. }));
    assert!(!base.italic.is_zero());
    assert!(italic_kern.width.eq_dim(&(-base.italic.clone())));
    let center = &(&base.height - &base.depth) / &Dim::from_i64(2);
    assert!((&center + &base.shift).eq_dim(&params.axis_height));
    let BoxContent::Overlap(branches) = &slot.content else {
        panic!("expected side scripts")
    };
    let [upper, lower] = branches.as_slice() else {
        panic!("expected two limits")
    };
    let upper_parts = children(upper);
    assert!(upper_parts[0].width.eq_dim(&base.italic));
    let constants = font.face().tables().math.unwrap().constants.unwrap();
    let fu = |value| Dim::from_font_units(i64::from(value), font.units_per_em());
    let base_height = (&base.height + &base.shift).clamp_nonneg();
    let base_depth = (&base.depth - &base.shift).clamp_nonneg();
    let min_rise = &base_height - &fu(constants.superscript_baseline_drop_max().value);
    let min_drop = &base_depth + &fu(constants.subscript_baseline_drop_min().value);
    assert!(
        upper.shift.cmp(&min_rise).unwrap().is_ge(),
        "upper limit ignores integral height"
    );
    assert!(
        (-lower.shift.clone()).cmp(&min_drop).unwrap().is_ge(),
        "lower limit ignores integral depth"
    );
}
