use super::{MAX_DEPTH, MAX_NODES, MAX_SOURCE, render};
use serde_json::{Value, json};

#[test]
fn accepts_the_display_catalog_without_rewriting_source_values() {
    let root = json!({"type":"stack","direction":"row","gap":"large","children":[
        {"type":"card","title":"Synthetic report","children":[
            {"type":"text","text":"Report","size":"large","tone":"accent","single_line":true,"mono":false},
            {"type":"metric","label":"Completed","value":"12","detail":"Synthetic data","tone":"success"},
            {"type":"table","columns":["Name","Value"],"rows":[["A","12"]]},
            {"type":"bar_chart","unit":"ms","bars":[{"label":"A","value":12.5,"tone":"#5a7fcd"}]}]},
        {"type":"tabs","tabs":[{"label":"Diagram","count":1,"children":[
            {"type":"svg","source":"<svg xmlns=\"http://www.w3.org/2000/svg\"/>","width":320,"height":180}]}]}]});
    assert_eq!(render("json-render", &root.to_string()).unwrap(), root);
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\" fill=\"red\"/></svg>";
    let image = render("svg", svg).unwrap();
    assert_eq!(image["source"], svg);
    assert_eq!(image["width"], 10.);
    assert_eq!(image["height"], 10.);
}

#[test]
fn raw_svg_uses_its_authored_dimensions_without_upscaling_or_letterboxing() {
    for (attributes, width, height) in [
        (r#"viewBox="0 0 300 460""#, 300., 460.),
        (r#"width="1200" height="300""#, 640., 160.),
        (r#"width="300" height="12000""#, 102.4, 4096.),
        (r#"width="2in" height="72pt""#, 192., 96.),
        (r#"width="25.4mm" height="2.54cm""#, 96., 96.),
        (r#"width="6pc" height="101.6Q""#, 96., 96.),
        (r#"width="120px" viewBox="-10,-20,300,600""#, 120., 240.),
        (r#"height="80" viewBox="0 0 300 600""#, 40., 80.),
        (r#"width="100%" viewBox="0 0 300 460""#, 300., 460.),
        (
            r#"width="200" height="100" viewBox="0 0 10 10""#,
            200.,
            100.,
        ),
    ] {
        let source = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" {attributes}/>"#);
        let image = render("svg", &source).unwrap();
        assert_eq!(image["source"], source);
        assert!(
            (image["width"].as_f64().unwrap() - width).abs() < 1e-9,
            "{attributes}"
        );
        assert!(
            (image["height"].as_f64().unwrap() - height).abs() < 1e-9,
            "{attributes}"
        );
    }
}

#[test]
fn svg_unknown_dimensions_keep_the_fallback_and_extreme_values_stay_valid() {
    for attributes in [
        "",
        r#"width="100%" height="50%""#,
        r#"viewBox="0 0 -1 40""#,
        r#"viewBox="0 0 30 40 50""#,
        r#"width="NaN" height="100""#,
    ] {
        let source = format!("<svg {attributes}/>");
        let image = render("svg", &source).unwrap();
        assert_eq!(image["width"], 640., "{attributes}");
        assert_eq!(image["height"], 360., "{attributes}");
    }
    for dimensions in ["0.001 20000", "1e300 1e-300"] {
        let image = render("svg", &format!(r#"<svg viewBox="0 0 {dimensions}"/>"#)).unwrap();
        for field in ["width", "height"] {
            let value = image[field].as_f64().unwrap();
            assert!(
                value.is_finite() && (1. ..=4096.).contains(&value),
                "{image}"
            );
        }
        assert!(image["width"].as_f64().unwrap() <= 640.);
    }
}

#[test]
fn malformed_unknown_and_actionable_data_falls_back_instead_of_becoming_controls() {
    for source in ["", "{", "[]", "null", "{\"type\":\"Text\",\"text\":\"x\"}"] {
        assert!(render("json-render", source).is_err(), "{source}");
    }
    for root in [
        json!({"type":"button","label":"Run","action":"run"}),
        json!({"type":"text","text":"Run","action":"run"}),
        json!({"type":"card","children":[],"on_click":{"command":"run"}}),
        json!({"type":"tabs","tabs":[],"action":"switch"}),
        json!({"type":"text","text":"x","href":"https://example.com"}),
        json!({"type":"table","columns":["A"],"rows":[["a","b"]]}),
        json!({"type":"bar_chart","bars":[{"label":"A","value":-1}]}),
        json!({"type":"svg","source":"<svg/>","width":0,"height":20}),
        json!({"type":"text","text":"x","tone":"#nothex"}),
        json!({"type":"text","text":"x","mono":"yes"}),
    ] {
        assert!(render("json-render", &root.to_string()).is_err(), "{root}");
    }
    assert!(render("html", "<div>text</div>").is_err());
    assert!(render("svg", " \n ").is_err());
}

#[test]
fn bounded_sources_depth_and_node_count_are_explicit() {
    assert!(render("svg", &" ".repeat(MAX_SOURCE + 1)).is_err());
    let mut root = json!({"type":"text","text":"leaf"});
    for _ in 0..=MAX_DEPTH {
        root = json!({"type":"stack","children":[root]});
    }
    assert!(render("json-render", &root.to_string()).is_err());
    let children: Vec<Value> = (0..MAX_NODES)
        .map(|_| json!({"type":"text","text":"x"}))
        .collect();
    assert!(
        render(
            "json-render",
            &json!({"type":"stack","children":children}).to_string()
        )
        .is_err()
    );
}
