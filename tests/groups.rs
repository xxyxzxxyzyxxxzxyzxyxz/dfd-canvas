use dfd_canvas::{
    editor::Editor,
    geometry::port_position,
    model::{Document, Group, GroupLayout, NodeKind, Port, SCHEMA_VERSION},
};
use std::collections::BTreeSet;

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|s| (*s).into()).collect()
}

fn grouped() -> (Editor, String, String, String) {
    let mut e = Editor::new(Document::default());
    let a = e.add_node(NodeKind::Process, 100.0, 120.0).unwrap();
    let b = e.add_node(NodeKind::DataStore, 700.0, 120.0).unwrap();
    e.add_edge(&a, &b, Port::Right, Port::Left).unwrap();
    let g = e.create_group(&ids(&[&a])).unwrap();
    e.document.layout.groups.get_mut(&g).unwrap().width = 500.0;
    e.document.layout.groups.get_mut(&g).unwrap().height = 340.0;
    e.document.validate().unwrap();
    e.replace(e.document.clone());
    (e, a, b, g)
}

fn move_node(e: &mut Editor, id: &str, x: f64, y: f64) -> Result<bool, String> {
    let before = e.begin();
    let r = e.document.layout.nodes.get_mut(id).unwrap();
    r.x = x;
    r.y = y;
    e.finish_layout(before, &ids(&[id]))
}

#[test]
fn legacy_v1_migrates_and_always_saves_v2() {
    let original = include_str!("fixtures/example.dfd.json");
    let document = Document::from_json(original).unwrap();
    assert_eq!(document.schema_version, SCHEMA_VERSION);
    assert!(document.graph.groups.is_empty());
    assert!(document.layout.groups.is_empty());
    assert!(document.graph.nodes.iter().all(|n| n.group_id.is_none()));
    let saved: serde_json::Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 2);
    assert_eq!(saved["graph"]["groups"], serde_json::json!([]));
    assert_eq!(saved["layout"]["groups"], serde_json::json!({}));
    assert_eq!(
        saved["graph"]["nodes"][0]["group_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        Document::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
}

#[test]
fn creating_group_with_padding_and_membership_is_one_undo() {
    let mut e = Editor::new(Document::default());
    let a = e.add_node(NodeKind::Process, 80.0, 80.0).unwrap();
    let b = e.add_node(NodeKind::Process, 320.0, 120.0).unwrap();
    e.replace(e.document.clone());
    let before = e.begin();
    let group = e.create_group(&ids(&[&a, &b])).unwrap();
    let content = e.document.layout.groups[&group].group_content();
    for n in &e.document.graph.nodes {
        assert_eq!(n.group_id.as_ref(), Some(&group));
        assert!(content.contains(e.document.layout.nodes[&n.id]));
    }
    let grouped = e.document.clone();
    assert!(e.undo());
    assert_eq!(e.document, before);
    assert!(!e.can_undo());
    assert!(e.redo());
    assert_eq!(e.document, grouped);
}

#[test]
fn create_rejects_unselected_nodes_and_existing_members() {
    let mut e = Editor::new(Document::default());
    let a = e.add_node(NodeKind::Process, 100.0, 100.0).unwrap();
    e.add_node(NodeKind::Process, 300.0, 100.0).unwrap();
    let c = e.add_node(NodeKind::Process, 500.0, 100.0).unwrap();
    e.replace(e.document.clone());
    let before = e.begin();
    assert!(e.create_group(&ids(&[&a, &c])).is_err());
    assert_eq!(e.document, before);
    assert!(!e.can_undo());
    assert!(e.create_group(&BTreeSet::new()).is_err());
    e.create_group(&ids(&[&a])).unwrap();
    let before = e.begin();
    assert!(e.create_group(&ids(&[&a])).is_err());
    assert_eq!(e.document, before);
}

#[test]
fn joining_leaving_and_transferring_change_semantics_once_per_drop() {
    let (mut e, a, b, g) = grouped();
    let h = e.create_group(&ids(&[&b])).unwrap();
    let initial = e.begin();
    let ai = e.document.ai_json().unwrap();
    move_node(&mut e, &b, 340.0, 220.0).unwrap();
    assert_eq!(e.document.graph.nodes[1].group_id.as_ref(), Some(&g));
    assert_ne!(e.document.ai_json().unwrap(), ai);
    assert!(e.document.layout.groups.contains_key(&h));
    e.undo();
    assert_eq!(e.document, initial);
    e.redo();
    move_node(&mut e, &b, 700.0, 500.0).unwrap();
    assert!(e.document.graph.nodes[1].group_id.is_none());
    move_node(&mut e, &a, 700.0, 120.0).unwrap();
    assert_eq!(e.document.graph.nodes[0].group_id.as_ref(), Some(&h));
    assert_eq!(e.document.graph.groups.len(), 2);
    assert!(
        e.document
            .graph
            .nodes
            .iter()
            .all(|n| n.group_id.as_ref() != Some(&g))
    );
    e.document.validate().unwrap();
}

#[test]
fn multi_node_drop_rejects_every_position_and_membership_atomically() {
    let (mut e, a, b, _) = grouped();
    let before = e.begin();
    e.document.layout.nodes.get_mut(&a).unwrap().x = 1000.0;
    e.document.layout.nodes.get_mut(&b).unwrap().x = 550.0;
    assert!(e.finish_layout(before.clone(), &ids(&[&a, &b])).is_err());
    assert_eq!(e.document, before);
    assert!(!e.can_undo());
}

#[test]
fn node_addition_uses_containment_and_rejects_header_padding_and_crossings() {
    let (mut e, _, _, g) = grouped();
    let added = e.add_node(NodeKind::Process, 340.0, 220.0).unwrap();
    assert_eq!(
        e.document.graph.nodes.last().unwrap().group_id.as_ref(),
        Some(&g)
    );
    assert!(e.undo());
    let before = e.begin();
    for (x, y) in [(550.0, 220.0), (100.0, 75.0), (90.0, 120.0)] {
        assert!(e.add_node(NodeKind::Process, x, y).is_err());
        assert_eq!(e.document, before);
        assert!(e.can_redo());
    }
    e.redo();
    assert!(e.document.layout.nodes.contains_key(&added));
}

#[test]
fn frame_moves_preserve_relative_positions_ports_and_ai_json() {
    let (mut e, a, _, g) = grouped();
    let before = e.begin();
    let ai = e.document.ai_json().unwrap();
    let port = port_position(e.document.layout.nodes[&a], Port::Right);
    for r in [
        e.document.layout.groups.get_mut(&g).unwrap(),
        e.document.layout.nodes.get_mut(&a).unwrap(),
    ] {
        r.x += 23.125;
        r.y += 14.375;
    }
    e.finish_layout(before.clone(), &BTreeSet::new()).unwrap();
    assert_eq!(e.document.ai_json().unwrap(), ai);
    let moved = port_position(e.document.layout.nodes[&a], Port::Right);
    assert_eq!(moved.x - port.x, 23.125);
    assert_eq!(moved.y - port.y, 14.375);
    assert!(e.undo());
    assert_eq!(e.document, before);
    assert!(!e.can_undo());
}

#[test]
fn frame_resize_rejects_member_overflow_and_capturing_unrelated_nodes() {
    let (mut e, _, _, g) = grouped();
    for width in [120.0, 800.0] {
        let before = e.begin();
        e.document.layout.groups.get_mut(&g).unwrap().width = width;
        assert!(e.finish_layout(before.clone(), &BTreeSet::new()).is_err());
        assert_eq!(e.document, before);
        assert!(!e.can_undo());
    }
    let before = e.begin();
    e.document.layout.groups.get_mut(&g).unwrap().width = 520.0;
    e.finish_layout(before.clone(), &BTreeSet::new()).unwrap();
    e.undo();
    assert_eq!(e.document, before);
}

#[test]
fn frame_move_rejects_newly_enclosed_nodes_without_changing_membership() {
    let (mut e, a, _, g) = grouped();
    let before = e.begin();
    e.document.layout.groups.get_mut(&g).unwrap().x += 500.0;
    e.document.layout.nodes.get_mut(&a).unwrap().x += 500.0;
    assert!(e.finish_layout(before.clone(), &BTreeSet::new()).is_err());
    assert_eq!(e.document, before);
}

#[test]
fn group_delete_preserves_nodes_edges_and_is_reversible() {
    let (mut e, _, _, g) = grouped();
    let before = e.begin();
    e.delete(&ids(&[&g]));
    assert!(e.document.graph.groups.is_empty());
    assert!(e.document.layout.groups.is_empty());
    assert_eq!(e.document.layout.nodes, before.layout.nodes);
    assert_eq!(e.document.graph.edges, before.graph.edges);
    assert!(e.document.graph.nodes.iter().all(|n| n.group_id.is_none()));
    e.document.validate().unwrap();
    e.undo();
    assert_eq!(e.document, before);
    e.redo();
    assert!(e.document.graph.groups.is_empty());
}

#[test]
fn deleting_last_member_keeps_empty_group_and_cascades_edges() {
    let (mut e, a, _, g) = grouped();
    let before = e.begin();
    e.delete(&ids(&[&a]));
    assert_eq!(e.document.graph.groups[0].id, g);
    assert_eq!(e.document.layout.groups, before.layout.groups);
    assert!(e.document.graph.edges.is_empty());
    e.document.validate().unwrap();
    e.undo();
    assert_eq!(e.document, before);
}

#[test]
fn invalid_group_loads_preserve_current_document_and_history() {
    let (mut e, a, b, g) = grouped();
    let h = e.create_group(&ids(&[&b])).unwrap();
    let before = e.begin();
    let mut invalid = Vec::new();
    let mut d = before.clone();
    d.graph.groups[0].id = a;
    invalid.push(d);
    let mut d = before.clone();
    d.graph.groups[1].id = g.clone();
    invalid.push(d);
    let mut d = before.clone();
    d.graph.nodes[0].group_id = Some("missing".into());
    invalid.push(d);
    let mut d = before.clone();
    d.graph.nodes[0].group_id = None;
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.remove(&g);
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.insert("orphan".into(), d.layout.groups[&g]);
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.get_mut(&h).unwrap().x = 500.0;
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.get_mut(&g).unwrap().y += 20.0;
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.get_mut(&g).unwrap().width = 0.0;
    invalid.push(d);
    let mut d = before.clone();
    d.layout.groups.get_mut(&g).unwrap().x = 1_000_001.0;
    invalid.push(d);
    let mut d = before.clone();
    d.schema_version = 1;
    invalid.push(d);
    for doc in invalid {
        assert!(e.load_json(&serde_json::to_string(&doc).unwrap()).is_err());
        assert_eq!(e.document, before);
        assert!(e.can_undo());
    }
}

#[test]
fn group_json_sorts_and_roundtrips_without_layout_in_semantics() {
    let (mut e, _, b, g) = grouped();
    e.create_group(&ids(&[&b])).unwrap();
    e.document.graph.groups[0].name = "リソースグループ 🗂".into();
    e.document.graph.groups[0].description = "日本語\n<script>plain text</script>".into();
    let expected = e.document.to_json().unwrap();
    let ai = e.document.ai_json().unwrap();
    e.document.graph.groups.reverse();
    assert_eq!(e.document.to_json().unwrap(), expected);
    assert_eq!(e.document.ai_json().unwrap(), ai);
    for _ in 0..5 {
        e.load_json(&e.document.to_json().unwrap()).unwrap();
    }
    assert_eq!(e.document.to_json().unwrap(), expected);
    let value: serde_json::Value = serde_json::from_str(&ai).unwrap();
    assert!(value.get("layout").is_none());
    assert!(value["graph"]["groups"][0].get("width").is_none());
    assert_eq!(value["graph"]["nodes"][0]["group_id"], g);
}

#[test]
fn allocator_checks_group_ids_even_with_node_prefix() {
    let mut doc = Document::default();
    doc.graph.groups.push(Group {
        id: "node-000001".into(),
        name: "空".into(),
        description: String::new(),
    });
    doc.layout.groups.insert(
        "node-000001".into(),
        GroupLayout {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 240.0,
        },
    );
    let mut e = Editor::new(doc);
    assert_ne!(
        e.add_node(NodeKind::Process, 600.0, 600.0).unwrap(),
        "node-000001"
    );
    e.document.validate().unwrap();
}

#[test]
fn exact_content_boundary_is_valid_but_partial_containment_is_not() {
    let (e, a, _, g) = grouped();
    let mut rect = e.document.layout.nodes[&a];
    let content = e.document.layout.groups[&g].group_content();
    rect.x = content.x + content.width - rect.width;
    rect.y = content.y + content.height - rect.height;
    assert_eq!(e.document.group_at(rect).unwrap(), Some(g));
    rect.x += 0.01;
    assert!(e.document.group_at(rect).is_err());
}
