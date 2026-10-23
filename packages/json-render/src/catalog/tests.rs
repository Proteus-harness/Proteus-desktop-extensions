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
    assert_eq!(image["width"], 640.);
    assert_eq!(image["height"], 360.);
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
