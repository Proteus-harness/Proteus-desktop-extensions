//! Semantic regressions from the 2026-11-07 static audit. This package remains opt-in.
use mermaid_rs_renderer::layout::DiagramData;
use mermaid_rs_renderer::{LayoutConfig, Theme, compute_layout, parse_mermaid_strict};

fn graph(source: &str) -> mermaid_rs_renderer::Graph {
    parse_mermaid_strict(source).unwrap().graph
}

#[test]
fn block_columns_are_bounded_before_allocation_and_labels_stay_whole() {
    assert!(parse_mermaid_strict("block-beta\n columns 1000000000\n A").is_err());
    let mut g = graph("block-beta\n columns 2\n A[\"Hello world\"] B[Other]");
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.nodes["A"].label, "Hello world");
    // Programmatic callers bypass parsing; layout still limits allocations.
    g.block.as_mut().unwrap().columns = Some(usize::MAX);
    let layout = compute_layout(&g, &Theme::modern(), &LayoutConfig::default());
    assert!(layout.width.is_finite());
}

#[test]
fn malformed_delimiters_do_not_panic() {
    for source in [
        "architecture-beta\n service a][",
        "architecture-beta\n service a)(",
        "flowchart TD\n A[/]",
        "flowchart TD\n A[\\]",
    ] {
        let _ = parse_mermaid_strict(source);
    }
}

#[test]
fn gitgraph_options_do_not_change_branch_identity_or_merge_parents() {
    let g = graph(
        "gitGraph\n commit id:\"base\"\n branch feature order: 1\n commit id:\"topic\"\n checkout main\n commit id:\"main\"\n merge feature id:\"M\"",
    );
    assert!(g.gitgraph.branches.iter().any(|b| b.name == "feature"));
    assert!(
        !g.gitgraph
            .branches
            .iter()
            .any(|b| b.name.contains("order:"))
    );
    let merge = g.gitgraph.commits.iter().find(|c| c.id == "M").unwrap();
    assert_eq!(merge.parents.len(), 2);
    assert!(merge.parents.iter().any(|id| id == "topic"));
    let g = graph(
        "gitGraph\n commit\n branch \"feature space\" order: 1\n commit id:\"topic\"\n checkout main\n commit\n merge \"feature space\" id:\"M\"",
    );
    assert!(
        g.gitgraph
            .branches
            .iter()
            .any(|b| b.name == "feature space")
    );
    assert_eq!(
        g.gitgraph
            .commits
            .iter()
            .find(|c| c.id == "M")
            .unwrap()
            .parents
            .len(),
        2
    );
}

#[test]
fn gantt_dependencies_preserve_case_and_standalone_classes_exist() {
    let g = graph("gantt\n Task A : A1, 2020-01-01, 5d\n Task B : after A1, 3d");
    assert_eq!(g.gantt_tasks[1].after.as_deref(), Some("A1"));
    assert!(!g.nodes.contains_key("a1"));
    let g = graph("classDiagram\n Animal : +String name");
    assert!(g.nodes["Animal"].label.contains("name"));
}

#[test]
fn timeline_continuations_and_sequence_activation_keep_their_owners() {
    let g = graph("timeline\n 2020 : Launch\n : Follow-up");
    assert_eq!(g.timeline.events[0].events, ["Launch", "Follow-up"]);
    let g =
        graph("sequenceDiagram\n participant A\n participant B\n A->>+B: request\n B-->>-A: reply");
    assert_eq!(g.sequence_activations.len(), 2);
    assert_eq!(g.sequence_activations[0].participant, "B");
    assert_eq!(g.sequence_activations[1].participant, "B");
}

#[test]
fn quoted_hyphens_and_equals_are_label_text() {
    let g = graph("flowchart TD\n A[\"before -- after\"]");
    assert_eq!(g.nodes["A"].label, "before -- after");
    let g = graph("C4Context\n Person(a, \"x=y\")");
    assert_eq!(g.c4.shapes[0].label, "x=y");
}

#[test]
fn dump_distinguishes_subgraphs_with_the_same_caption() {
    let mut g =
        graph("flowchart TD\n subgraph first[Same]\n A\n end\n subgraph second[Same]\n B\n end");
    // IR can represent wrappers with the same label and membership, too.
    g.subgraphs[1].nodes = g.subgraphs[0].nodes.clone();
    let layout = compute_layout(&g, &Theme::modern(), &LayoutConfig::default());
    let dump = mermaid_rs_renderer::layout_dump::LayoutDump::from_layout(&layout, &g);
    let ids = dump
        .subgraphs
        .iter()
        .filter_map(|sub| sub.id.as_deref())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"first"));
    assert!(ids.contains(&"second"));
}

#[test]
fn disconnected_sankey_nodes_do_not_overlap_in_any_rank() {
    let g = graph("sankey-beta\n A,B,1\n C,D,1");
    let layout = compute_layout(&g, &Theme::modern(), &LayoutConfig::default());
    let DiagramData::Sankey(data) = layout.diagram else {
        panic!("not sankey")
    };
    for rank in 0..=1 {
        let mut nodes = data
            .nodes
            .iter()
            .filter(|node| node.rank == rank)
            .collect::<Vec<_>>();
        nodes.sort_by(|a, b| a.y.total_cmp(&b.y));
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].y + nodes[0].height < nodes[1].y);
        assert!(nodes[1].y + nodes[1].height <= data.height + 0.01);
    }
}

#[test]
fn signed_xy_bars_share_a_zero_baseline() {
    let g = graph("xychart-beta\n x-axis [a, b]\n y-axis -10 --> 10\n bar [-10, 10]");
    let layout = compute_layout(&g, &Theme::modern(), &LayoutConfig::default());
    let DiagramData::XYChart(data) = layout.diagram else {
        panic!("not xy")
    };
    assert_eq!(data.bars.len(), 2);
    let (negative, positive) = (&data.bars[0], &data.bars[1]);
    assert!((negative.height - positive.height).abs() < 0.01);
    assert!((positive.y + positive.height - negative.y).abs() < 0.01);
    assert!(negative.height > 0.0);
}
