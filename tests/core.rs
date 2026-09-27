use dfd_canvas::editor::{EdgeEnd, Editor, HISTORY_LIMIT};
use dfd_canvas::geometry::{
    Point, bezier_bounds, bezier_path, fit_view, port_position, screen_to_world, world_to_screen,
};
use dfd_canvas::model::{
    Document, EdgeLayout, MAX_FILE_BYTES, NodeKind, NodeLayout, Port, Viewport,
};
use std::collections::BTreeSet;

fn example() -> (Editor, String, String, String) {
    let mut editor = Editor::new(Document::default());
    let a = editor
        .add_node(NodeKind::ExternalEntity, 20.0, 40.0)
        .unwrap();
    let b = editor.add_node(NodeKind::Process, 320.0, 60.0).unwrap();
    let edge = editor.add_edge(&a, &b, Port::Right, Port::Left).unwrap();
    (editor, a, b, edge)
}

#[test]
fn empty_document_is_valid_and_round_trips() {
    let doc = Document::default();
    assert!(doc.validate().is_ok());
    assert_eq!(Document::from_json(&doc.to_json().unwrap()).unwrap(), doc);
}

#[test]
fn node_creation_normalizes_coordinates_to_document_bounds() {
    let mut editor = Editor::new(Document::default());
    editor
        .add_node(NodeKind::Process, f64::NAN, f64::INFINITY)
        .unwrap();
    editor
        .add_node(NodeKind::Process, -9_000_000.0, 9_000_000.0)
        .unwrap();
    assert!(editor.document.validate().is_ok());
}

#[test]
fn three_node_kinds_and_directed_edges_are_added() {
    let (mut editor, a, b, edge) = example();
    let c = editor.add_node(NodeKind::DataStore, 650.0, 40.0).unwrap();
    assert_eq!(editor.document.graph.nodes.len(), 3);
    assert_eq!(editor.document.graph.edges[0].id, edge);
    assert_eq!(editor.document.graph.edges[0].source, a);
    assert_eq!(editor.document.graph.edges[0].target, b);
    assert_eq!(editor.document.graph.nodes[2].id, c);
    assert!(editor.document.validate().is_ok());
}

#[test]
fn independent_ids_survive_renaming_and_moving() {
    let (mut editor, a, _, edge) = example();
    editor.document.graph.nodes[0].name = "注文受付".into();
    editor.document.graph.nodes[0].description = "日本語の説明\n二行目".into();
    editor.document.layout.nodes.get_mut(&a).unwrap().x = 800.0;
    assert_eq!(editor.document.graph.nodes[0].id, a);
    assert_eq!(editor.document.graph.edges[0].id, edge);
    assert_eq!(editor.document.graph.edges[0].source, a);
    assert!(editor.document.validate().is_ok());
}

#[test]
fn node_deletion_cascades_and_undo_redo_restores_identical_ids() {
    let (mut editor, a, b, edge) = example();
    let complete = editor.document.clone();
    assert!(editor.delete(&BTreeSet::from([a.clone()])));
    assert_eq!(editor.document.graph.nodes.len(), 1);
    assert_eq!(editor.document.graph.nodes[0].id, b);
    assert!(editor.document.graph.edges.is_empty());
    assert!(!editor.document.layout.nodes.contains_key(&a));
    assert!(!editor.document.layout.edges.contains_key(&edge));
    assert!(editor.document.validate().is_ok());
    assert!(editor.undo());
    assert_eq!(editor.document, complete);
    assert!(editor.redo());
    assert_eq!(editor.document.graph.nodes.len(), 1);
    assert!(editor.document.graph.edges.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.document, complete);
}

#[test]
fn deleting_edge_preserves_its_nodes() {
    let (mut editor, _, _, edge) = example();
    assert!(editor.delete(&BTreeSet::from([edge])));
    assert_eq!(editor.document.graph.nodes.len(), 2);
    assert!(editor.document.graph.edges.is_empty());
    assert!(editor.document.layout.edges.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.document.graph.edges.len(), 1);
}

#[test]
fn multi_selection_deletion_is_one_history_operation() {
    let (mut editor, a, b, _) = example();
    let before = editor.document.clone();
    assert!(editor.delete(&BTreeSet::from([a, b])));
    assert!(editor.document.graph.nodes.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.document, before);
}

#[test]
fn drag_and_resize_with_many_updates_commit_once() {
    let (mut editor, a, b, _) = example();
    // Start a fresh history to make the number of undo steps observable.
    editor.replace(editor.document.clone());
    let before = editor.begin();
    for _ in 0..100 {
        editor.document.layout.nodes.get_mut(&a).unwrap().x += 1.0;
        editor.document.layout.nodes.get_mut(&b).unwrap().y += 0.5;
    }
    assert!(!editor.can_undo());
    assert!(editor.commit(before.clone()));
    assert!(editor.undo());
    assert_eq!(editor.document, before);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    let before_resize = editor.begin();
    for _ in 0..20 {
        let rect = editor.document.layout.nodes.get_mut(&a).unwrap();
        rect.width += 3.0;
        rect.height += 2.0;
    }
    assert!(editor.commit(before_resize.clone()));
    assert!(editor.undo());
    assert_eq!(editor.document, before_resize);
}

#[test]
fn cancelled_operation_can_restore_snapshot_without_history() {
    let (mut editor, a, _, _) = example();
    editor.replace(editor.document.clone());
    let before = editor.begin();
    editor.document.layout.nodes.get_mut(&a).unwrap().x = 900.0;
    editor.document = before.clone();
    assert!(!editor.commit(before));
    assert!(!editor.can_undo());
}

#[test]
fn new_edit_after_undo_discards_redo_and_never_reuses_allocated_ids() {
    let mut editor = Editor::new(Document::default());
    let old_id = editor.add_node(NodeKind::Process, 0.0, 0.0).unwrap();
    assert!(editor.undo());
    assert!(editor.can_redo());
    let new_id = editor.add_node(NodeKind::Process, 0.0, 0.0).unwrap();
    assert_ne!(new_id, old_id);
    assert!(!editor.can_redo());
    assert!(!editor.redo());
}

#[test]
fn no_op_commit_does_not_discard_redo() {
    let (mut editor, _, _, _) = example();
    editor.undo();
    assert!(!editor.commit(editor.begin()));
    assert!(editor.can_redo());
}

#[test]
fn history_limit_is_one_hundred_operations() {
    let mut editor = Editor::new(Document::default());
    for index in 0..HISTORY_LIMIT + 5 {
        let before = editor.begin();
        editor.document.name = format!("文書{index}");
        editor.commit(before);
    }
    let mut undos = 0;
    while editor.undo() {
        undos += 1;
    }
    assert_eq!(undos, HISTORY_LIMIT);
    assert_eq!(editor.document.name, "文書4");
    let mut redos = 0;
    while editor.redo() {
        redos += 1;
    }
    assert_eq!(redos, HISTORY_LIMIT);
}

#[test]
fn allocator_avoids_loaded_ids() {
    let (editor, _, _, _) = example();
    let mut loaded = Editor::new(editor.document);
    loaded.add_node(NodeKind::Process, 0.0, 0.0).unwrap();
    loaded.add_node(NodeKind::Process, 0.0, 0.0).unwrap();
    assert!(loaded.document.validate().is_ok());
}

#[test]
fn parallel_reverse_and_cyclic_edges_remain_independent() {
    let (mut editor, a, b, first) = example();
    let parallel = editor.add_edge(&a, &b, Port::Right, Port::Left).unwrap();
    let reverse = editor.add_edge(&b, &a, Port::Left, Port::Right).unwrap();
    let c = editor.add_node(NodeKind::DataStore, 640.0, 40.0).unwrap();
    editor.add_edge(&b, &c, Port::Right, Port::Left).unwrap();
    editor.add_edge(&c, &a, Port::Bottom, Port::Bottom).unwrap();
    assert_ne!(first, parallel);
    assert_ne!(parallel, reverse);
    assert_eq!(editor.document.graph.edges.len(), 5);
    assert!(editor.document.validate().is_ok());
}

#[test]
fn invalid_edge_creation_leaves_document_and_history_unchanged() {
    let (mut editor, a, _, _) = example();
    editor.replace(editor.document.clone());
    let before = editor.begin();
    assert!(editor.add_edge(&a, &a, Port::Top, Port::Bottom).is_err());
    assert!(
        editor
            .add_edge(&a, "missing", Port::Top, Port::Bottom)
            .is_err()
    );
    assert_eq!(editor.document, before);
    assert!(!editor.can_undo());
}

#[test]
fn relocating_either_edge_endpoint_preserves_graph_and_other_edges() {
    let (mut editor, a, b, edge) = example();
    editor.document.graph.edges[0].name = "注文データ".into();
    editor.document.graph.edges[0].description = "受注処理への入力".into();
    editor.add_edge(&a, &b, Port::Right, Port::Left).unwrap();
    editor.add_edge(&b, &a, Port::Top, Port::Bottom).unwrap();
    let original = editor.document;
    let semantic = original.ai_json().unwrap();
    for end in [EdgeEnd::Source, EdgeEnd::Target] {
        for port in [Port::Top, Port::Right, Port::Bottom, Port::Left] {
            let mut editor = Editor::new(original.clone());
            let mut expected = original.clone();
            let layout = expected.layout.edges.get_mut(&edge).unwrap();
            match end {
                EdgeEnd::Source => layout.source_port = port,
                EdgeEnd::Target => layout.target_port = port,
            }
            assert_eq!(
                editor.set_edge_port(&edge, end, port).unwrap(),
                expected != original
            );
            assert_eq!(editor.document, expected);
            assert_eq!(editor.document.ai_json().unwrap(), semantic);
            assert_eq!(
                Document::from_json(&editor.document.to_json().unwrap()).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn relocating_each_edge_endpoint_is_one_undoable_operation() {
    let (editor, _, _, edge) = example();
    let original = editor.document;
    for end in [EdgeEnd::Source, EdgeEnd::Target] {
        let mut editor = Editor::new(original.clone());
        assert!(editor.set_edge_port(&edge, end, Port::Top).unwrap());
        let relocated = editor.document.clone();
        assert!(editor.undo());
        assert_eq!(editor.document, original);
        assert!(!editor.can_undo());
        assert!(editor.redo());
        assert_eq!(editor.document, relocated);
        assert!(!editor.can_redo());
    }
}

#[test]
fn unchanged_edge_ports_preserve_document_and_redo_history() {
    let (editor, _, _, edge) = example();
    let mut editor = Editor::new(editor.document);
    let original = editor.document.clone();
    assert!(
        editor
            .set_edge_port(&edge, EdgeEnd::Source, Port::Top)
            .unwrap()
    );
    let relocated = editor.document.clone();
    assert!(editor.undo());
    for (end, port) in [
        (EdgeEnd::Source, Port::Right),
        (EdgeEnd::Target, Port::Left),
    ] {
        assert!(!editor.set_edge_port(&edge, end, port).unwrap());
        assert_eq!(editor.document, original);
        assert!(!editor.can_undo());
        assert!(editor.can_redo());
    }
    assert!(editor.redo());
    assert_eq!(editor.document, relocated);
}

#[test]
fn invalid_edge_port_changes_preserve_document_and_history() {
    let (editor, a, _, edge) = example();
    let mut editor = Editor::new(editor.document);
    assert!(
        editor
            .set_edge_port(&edge, EdgeEnd::Target, Port::Bottom)
            .unwrap()
    );
    let relocated = editor.document.clone();
    assert!(editor.undo());
    let original = editor.document.clone();
    for id in ["missing", &a] {
        for end in [EdgeEnd::Source, EdgeEnd::Target] {
            assert!(editor.set_edge_port(id, end, Port::Top).is_err());
            assert_eq!(editor.document, original);
            assert!(!editor.can_undo());
            assert!(editor.can_redo());
        }
    }
    assert!(editor.redo());
    assert_eq!(editor.document, relocated);

    // A malformed document must not turn a missing layout into a panic or edit.
    editor.document.layout.edges.remove(&edge);
    editor.replace(editor.document.clone());
    let without_layout = editor.document.clone();
    assert!(
        editor
            .set_edge_port(&edge, EdgeEnd::Source, Port::Top)
            .is_err()
    );
    assert_eq!(editor.document, without_layout);
    assert!(!editor.can_undo());
}

#[test]
fn json_round_trip_preserves_japanese_and_plain_text() {
    let (mut editor, _, _, _) = example();
    editor.document.name = "受注システム".into();
    editor.document.graph.nodes[0].name = "顧客<script>alert(1)</script>".into();
    editor.document.graph.nodes[0].description = "😀 日本語\n複数行\"引用\"".into();
    editor.document.graph.edges[0].name = "注文データ".into();
    editor.document.layout.viewport = Viewport {
        pan_x: 36.0,
        pan_y: -19.0,
        zoom: 1.7,
    };
    let json = editor.document.to_json().unwrap();
    assert_eq!(Document::from_json(&json).unwrap(), editor.document);
    assert!(!json.contains("undo"));
    assert!(!json.contains("selection"));
}

#[test]
fn repeated_json_round_trips_preserve_exact_fractional_browser_layout() {
    let (mut editor, a, b, _) = example();
    // Actual coordinates recorded after zoom, drag, and resize in the browser.
    // The width used to deserialize one ULP lower without float_roundtrip.
    editor.document.layout.nodes.insert(
        a.clone(),
        NodeLayout {
            x: 158.33333333333331,
            y: 173.51302083333331,
            width: 214.16666666666674,
            height: 105.0,
        },
    );
    editor.document.layout.nodes.get_mut(&b).unwrap().y = 320.1796875;
    editor.document.layout.viewport = Viewport {
        pan_x: -36.39999999999998,
        pan_y: -124.43593750000002,
        zoom: 1.2,
    };
    let expected = editor.document.clone();
    let expected_json = expected.to_json().unwrap();
    let expected_ai = expected.ai_json().unwrap();
    for _ in 0..10 {
        editor
            .load_json(&editor.document.to_json().unwrap())
            .unwrap();
        for (id, original) in &expected.layout.nodes {
            let restored = editor.document.layout.nodes[id];
            for (before, after) in [
                (original.x, restored.x),
                (original.y, restored.y),
                (original.width, restored.width),
                (original.height, restored.height),
            ] {
                assert_eq!(before.to_bits(), after.to_bits(), "layout drift for {id}");
            }
        }
        let original = expected.layout.viewport;
        let restored = editor.document.layout.viewport;
        for (before, after) in [
            (original.pan_x, restored.pan_x),
            (original.pan_y, restored.pan_y),
            (original.zoom, restored.zoom),
        ] {
            assert_eq!(before.to_bits(), after.to_bits(), "viewport drift");
        }
        assert_eq!(editor.document.to_json().unwrap(), expected_json);
        assert_eq!(editor.document.ai_json().unwrap(), expected_ai);
    }
}

#[test]
fn fractional_zoom_viewports_round_trip_without_changing_pointer_coordinates() {
    let (mut editor, _, _, _) = example();
    for zoom in [
        0.10000000000000002,
        0.3333333333333333,
        1.0000000000000002,
        1.2,
        3.9999999999999996,
    ] {
        editor.document.layout.viewport = Viewport {
            pan_x: 491.375 - 317.3333333333333 * zoom,
            pan_y: 271.125 - 173.51302083333331 * zoom,
            zoom,
        };
        let screen = Point {
            x: 667.25,
            y: 381.75,
        };
        let before = screen_to_world(screen, editor.document.layout.viewport);
        let restored = Document::from_json(&editor.document.to_json().unwrap()).unwrap();
        let after = screen_to_world(screen, restored.layout.viewport);
        assert_eq!(before.x.to_bits(), after.x.to_bits());
        assert_eq!(before.y.to_bits(), after.y.to_bits());
        assert_eq!(zoom.to_bits(), restored.layout.viewport.zoom.to_bits());
    }
}

#[test]
fn invalid_load_preserves_document_and_history() {
    let (mut editor, _, _, _) = example();
    let before = editor.begin();
    for input in ["not json", "{}", "null", "{\"schema_version\":99}"] {
        assert!(editor.load_json(input).is_err());
        assert_eq!(editor.document, before);
        assert!(editor.can_undo());
    }
    let mut invalid = before.clone();
    invalid.graph.edges[0].target = "missing".into();
    assert!(
        editor
            .load_json(&serde_json::to_string(&invalid).unwrap())
            .is_err()
    );
    assert_eq!(editor.document, before);
}

#[test]
fn oversized_json_is_rejected_before_parsing() {
    let input = " ".repeat(MAX_FILE_BYTES + 1);
    assert!(Document::from_json(&input).unwrap_err().contains("5 MiB"));
}

#[test]
fn unsupported_schema_kind_port_and_unknown_fields_are_rejected() {
    let (editor, _, _, _) = example();
    let json = editor.document.to_json().unwrap();
    assert!(
        Document::from_json(&json.replace("\"schema_version\": 2", "\"schema_version\": 99"))
            .is_err()
    );
    assert!(Document::from_json(&json.replace("\"process\"", "\"unknown_kind\"")).is_err());
    assert!(Document::from_json(&json.replace("\"right\"", "\"north\"")).is_err());
    assert!(Document::from_json(&json.replacen('{', "{\"extra\":true,", 1)).is_err());
}

#[test]
fn duplicate_ids_and_dangling_references_are_rejected() {
    let (editor, a, _, _) = example();
    let mut doc = editor.document.clone();
    doc.graph.nodes[1].id = a.clone();
    assert!(doc.validate().unwrap_err().contains("重複"));
    let mut doc = editor.document.clone();
    doc.graph.edges[0].id = a;
    assert!(doc.validate().unwrap_err().contains("重複"));
    let mut doc = editor.document.clone();
    doc.graph.edges[0].source = "missing".into();
    assert!(doc.validate().unwrap_err().contains("存在しません"));
    let mut doc = editor.document;
    doc.graph.edges[0].target = doc.graph.edges[0].source.clone();
    assert!(doc.validate().unwrap_err().contains("自己接続"));
}

#[test]
fn missing_or_orphan_layout_is_rejected() {
    let (editor, a, _, edge) = example();
    let mut doc = editor.document.clone();
    doc.layout.nodes.remove(&a);
    assert!(doc.validate().is_err());
    let mut doc = editor.document.clone();
    doc.layout.edges.remove(&edge);
    assert!(doc.validate().is_err());
    let mut doc = editor.document.clone();
    doc.layout
        .nodes
        .insert("orphan".into(), doc.layout.nodes[&a]);
    assert!(doc.validate().is_err());
    let mut doc = editor.document;
    doc.layout.edges.insert(
        "orphan".into(),
        EdgeLayout {
            source_port: Port::Top,
            target_port: Port::Left,
        },
    );
    assert!(doc.validate().is_err());
}

#[test]
fn invalid_coordinates_dimensions_and_viewport_are_rejected() {
    let (editor, a, _, _) = example();
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        1_000_001.0,
        -1_000_001.0,
    ] {
        let mut doc = editor.document.clone();
        doc.layout.nodes.get_mut(&a).unwrap().x = value;
        assert!(doc.validate().is_err());
        let mut doc = editor.document.clone();
        doc.layout.viewport.pan_y = value;
        assert!(doc.validate().is_err());
    }
    for value in [-1.0, 0.0, 79.0, 10_001.0, f64::NAN] {
        let mut doc = editor.document.clone();
        doc.layout.nodes.get_mut(&a).unwrap().width = value;
        assert!(doc.validate().is_err());
    }
    for value in [-1.0, 0.0, 47.0, 10_001.0, f64::NAN] {
        let mut doc = editor.document.clone();
        doc.layout.nodes.get_mut(&a).unwrap().height = value;
        assert!(doc.validate().is_err());
    }
    for value in [0.0, 0.09, 4.01, f64::NAN, f64::INFINITY] {
        let mut doc = editor.document.clone();
        doc.layout.viewport.zoom = value;
        assert!(doc.validate().is_err());
    }
}

#[test]
fn ai_json_does_not_change_with_any_layout_edit_or_collection_order() {
    let (mut editor, a, b, edge) = example();
    editor.add_edge(&b, &a, Port::Top, Port::Bottom).unwrap();
    let before = editor.document.ai_json().unwrap();
    let rect = editor.document.layout.nodes.get_mut(&a).unwrap();
    rect.x = -240.0;
    rect.y = 720.0;
    rect.width = 310.0;
    rect.height = 260.0;
    editor.document.layout.viewport = Viewport {
        pan_x: 180.0,
        pan_y: -60.0,
        zoom: 2.0,
    };
    editor
        .document
        .layout
        .edges
        .get_mut(&edge)
        .unwrap()
        .source_port = Port::Top;
    editor.document.graph.nodes.reverse();
    editor.document.graph.edges.reverse();
    assert_eq!(editor.document.ai_json().unwrap(), before);
    let value: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert!(value.get("layout").is_none());
    assert!(value["graph"]["nodes"][0].get("x").is_none());
    assert!(value["graph"]["edges"][0].get("source_port").is_none());
    assert_eq!(value["graph"]["edges"][0]["source"], a);
    assert_eq!(value["graph"]["edges"][0]["target"], b);
}

#[test]
fn saving_sorts_without_mutating_document_order() {
    let (mut editor, _, _, _) = example();
    let expected = editor.document.to_json().unwrap();
    editor.document.graph.nodes.reverse();
    let before = editor.document.clone();
    assert_eq!(editor.document.to_json().unwrap(), expected);
    assert_eq!(editor.document, before);
}

#[test]
fn pan_zoom_coordinate_conversion_round_trips() {
    for zoom in [0.1, 0.25, 1.0, 1.75, 4.0] {
        let view = Viewport {
            pan_x: 247.0,
            pan_y: -137.0,
            zoom,
        };
        let world = Point {
            x: -49.25,
            y: 280.5,
        };
        let roundtrip = screen_to_world(world_to_screen(world, view), view);
        assert!((roundtrip.x - world.x).abs() < 1e-9);
        assert!((roundtrip.y - world.y).abs() < 1e-9);
        let screen = Point { x: 607.0, y: 80.0 };
        let roundtrip = world_to_screen(screen_to_world(screen, view), view);
        assert!((roundtrip.x - screen.x).abs() < 1e-9);
        assert!((roundtrip.y - screen.y).abs() < 1e-9);
    }
}

#[test]
fn node_ports_follow_movement_and_size() {
    let rect = NodeLayout {
        x: 20.0,
        y: 50.0,
        width: 160.0,
        height: 80.0,
    };
    assert_eq!(port_position(rect, Port::Top), Point { x: 100.0, y: 50.0 });
    assert_eq!(
        port_position(rect, Port::Right),
        Point { x: 180.0, y: 90.0 }
    );
    assert_eq!(
        port_position(rect, Port::Bottom),
        Point { x: 100.0, y: 130.0 }
    );
    assert_eq!(port_position(rect, Port::Left), Point { x: 20.0, y: 90.0 });
    let moved = NodeLayout {
        x: 80.0,
        y: 110.0,
        width: 240.0,
        height: 120.0,
    };
    assert_eq!(
        port_position(moved, Port::Right),
        Point { x: 320.0, y: 170.0 }
    );
}

#[test]
fn bezier_keeps_exact_endpoints_and_offsets_parallel_labels() {
    let a = Point { x: 10.0, y: 20.0 };
    let b = Point { x: 210.0, y: 20.0 };
    let (path, label) = bezier_path(a, Port::Right, b, Port::Left, 0.0);
    assert!(path.starts_with("M 10 20 C "));
    assert!(path.ends_with(", 210 20"));
    assert_eq!(label, Point { x: 110.0, y: 20.0 });
    let (_, offset_label) = bezier_path(a, Port::Right, b, Port::Left, 40.0);
    assert_eq!(offset_label, Point { x: 110.0, y: 50.0 });
    let (path, label) = bezier_path(a, Port::Right, a, Port::Left, 40.0);
    assert!(!path.contains("NaN"));
    assert!(label.x.is_finite() && label.y.is_finite());
}

#[test]
fn bezier_bounds_include_rendered_controls_and_label_for_every_port() {
    let a = Point {
        x: 1000.0,
        y: 1500.0,
    };
    let b = Point {
        x: 1020.0,
        y: 1520.0,
    };
    let ports = [Port::Top, Port::Right, Port::Bottom, Port::Left];
    for source_port in ports {
        for target_port in ports {
            for offset in [-360.0, 0.0, 360.0] {
                let bounds = bezier_bounds(a, source_port, b, target_port, offset);
                let (path, label) = bezier_path(a, source_port, b, target_port, offset);
                let numbers: Vec<f64> = path
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter_map(|token| token.parse().ok())
                    .collect();
                assert_eq!(numbers.len(), 8);
                let mut points: Vec<_> = numbers
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| Point { x: p[0], y: p[1] })
                    .collect();
                points.extend([
                    Point {
                        x: label.x - 70.0,
                        y: label.y - 13.0,
                    },
                    Point {
                        x: label.x + 70.0,
                        y: label.y + 13.0,
                    },
                ]);
                for point in points {
                    assert!(point.x >= bounds.x - 1e-9 && point.y >= bounds.y - 1e-9);
                    assert!(point.x <= bounds.x + bounds.width + 1e-9);
                    assert!(point.y <= bounds.y + bounds.height + 1e-9);
                }
                // The bounds must follow a translated diagram, not include (0, 0).
                assert!(bounds.x > 500.0 && bounds.y > 1000.0);
            }
        }
    }
}

#[test]
fn fit_view_includes_curves_above_nodes_and_their_labels() {
    let nodes = [
        NodeLayout {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 80.0,
        },
        NodeLayout {
            x: 440.0,
            y: 0.0,
            width: 160.0,
            height: 80.0,
        },
    ];
    let source = port_position(nodes[0], Port::Top);
    let target = port_position(nodes[1], Port::Top);
    let edge = bezier_bounds(source, Port::Top, target, Port::Top, 0.0);
    let (_, label) = bezier_path(source, Port::Top, target, Port::Top, 0.0);
    let node_only = fit_view(nodes, 800.0, 300.0);
    assert!(world_to_screen(label, node_only).y < 0.0);
    let all_bounds = [nodes[0], nodes[1], edge];
    let fitted = fit_view(all_bounds, 800.0, 300.0);
    for rect in all_bounds {
        let top_left = world_to_screen(
            Point {
                x: rect.x,
                y: rect.y,
            },
            fitted,
        );
        let bottom_right = world_to_screen(
            Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            },
            fitted,
        );
        assert!(top_left.x >= 47.9 && top_left.y >= 47.9);
        assert!(bottom_right.x <= 752.1 && bottom_right.y <= 252.1);
    }
}

#[test]
fn fit_view_centers_and_contains_normal_bounds() {
    let rects = [
        NodeLayout {
            x: -400.0,
            y: 500.0,
            width: 160.0,
            height: 80.0,
        },
        NodeLayout {
            x: 1200.0,
            y: -600.0,
            width: 240.0,
            height: 160.0,
        },
    ];
    let view = fit_view(rects, 1000.0, 800.0);
    for rect in rects {
        let top_left = world_to_screen(
            Point {
                x: rect.x,
                y: rect.y,
            },
            view,
        );
        let bottom_right = world_to_screen(
            Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            },
            view,
        );
        assert!(top_left.x >= 47.9 && top_left.y >= 47.9);
        assert!(bottom_right.x <= 952.1 && bottom_right.y <= 752.1);
    }
    assert_eq!(fit_view([], 1000.0, 800.0), Viewport::default());
}

#[test]
fn fit_view_keeps_far_valid_nodes_visible_and_viewport_valid() {
    for x in [-1_000_000.0, 1_000_000.0] {
        for y in [-1_000_000.0, 1_000_000.0] {
            let mut editor = Editor::new(Document::default());
            let id = editor.add_node(NodeKind::Process, x, y).unwrap();
            let rect = editor.document.layout.nodes[&id];
            let view = fit_view([rect], 800.0, 600.0);
            editor.document.layout.viewport = view;
            assert!(editor.document.validate().is_ok());
            let top_left = world_to_screen(
                Point {
                    x: rect.x,
                    y: rect.y,
                },
                view,
            );
            let bottom_right = world_to_screen(
                Point {
                    x: rect.x + rect.width,
                    y: rect.y + rect.height,
                },
                view,
            );
            assert!(top_left.x >= 47.9 && top_left.y >= 47.9);
            assert!(bottom_right.x <= 752.1 && bottom_right.y <= 552.1);
            assert!(((top_left.x + bottom_right.x) / 2.0 - 400.0).abs() < 1e-9);
            assert!(((top_left.y + bottom_right.y) / 2.0 - 300.0).abs() < 1e-9);
        }
    }
}
