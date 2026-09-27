use anyhow::{Result, ensure};
use dfd_canvas::model::{Document, Edge, EdgeLayout, Node, NodeKind, NodeLayout, Port, Viewport};

/// Generate the same deterministic graphs used by the browser performance suite.
pub fn make_document(count: usize, edge_count: usize) -> Result<Document> {
    ensure!(count >= 2, "At least two nodes are required");
    let mut document = Document {
        name: format!("性能確認 {count} ノード / {edge_count} エッジ"),
        ..Document::default()
    };
    document.layout.viewport = Viewport {
        pan_x: 20.0,
        pan_y: 20.0,
        zoom: 1.0,
    };
    let kinds = [
        NodeKind::Process,
        NodeKind::DataStore,
        NodeKind::ExternalEntity,
    ];
    let columns = if count <= 100 { 10 } else { 25 };
    for i in 0..count {
        let id = format!("n-{i:04}");
        document.layout.nodes.insert(
            id.clone(),
            NodeLayout {
                x: (40 + (i % columns) * 240) as f64,
                y: (40 + (i / columns) * 160) as f64,
                width: 170.0,
                height: 80.0,
            },
        );
        document.graph.nodes.push(Node {
            id,
            group_id: None,
            kind: kinds[i % kinds.len()],
            name: format!("業務ノード {}", i + 1),
            description: format!("性能確認用の説明 {}", i + 1),
        });
    }
    for i in 0..edge_count {
        let source = i % count;
        let target = (source + 1 + (i / count) % (count - 1)) % count;
        let id = format!("e-{i:04}");
        document.layout.edges.insert(
            id.clone(),
            EdgeLayout {
                source_port: Port::Right,
                target_port: Port::Left,
            },
        );
        document.graph.edges.push(Edge {
            id,
            source: document.graph.nodes[source].id.clone(),
            target: document.graph.nodes[target].id.clone(),
            name: format!("データ {}", i + 1),
            description: "ブラウザ性能確認用".into(),
        });
    }
    Ok(document)
}

/// Five resources per frame, with enough vertical space for real node gestures.
pub fn make_grouped_document(count: usize, edge_count: usize) -> Result<Document> {
    let mut document = make_document(count, edge_count)?;
    document.layout.viewport.pan_y = 80.0;
    for rect in document.layout.nodes.values_mut() {
        rect.y = 40.0 + (rect.y - 40.0) * 1.5;
    }
    let members: Vec<_> = document.graph.nodes.iter().map(|n| n.id.clone()).collect();
    let mut editor = dfd_canvas::editor::Editor::new(document);
    for chunk in members.chunks(5) {
        let group = editor
            .create_group(&chunk.iter().cloned().collect())
            .map_err(anyhow::Error::msg)?;
        let rect = editor.document.layout.groups.get_mut(&group).unwrap();
        rect.y -= 30.0;
        rect.height += 60.0;
    }
    editor.document.validate().map_err(anyhow::Error::msg)?;
    Ok(editor.document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_matches_graph_and_layout_rules_and_roundtrips() {
        for (nodes, edges) in [(2, 8), (100, 200), (500, 1_000)] {
            let document = make_document(nodes, edges).unwrap();
            assert_eq!(document.graph.nodes.len(), nodes);
            assert_eq!(document.graph.edges.len(), edges);
            document.validate().unwrap();
            assert_eq!(
                Document::from_json(&document.to_json().unwrap()).unwrap(),
                document
            );
            assert_eq!(document, make_document(nodes, edges).unwrap());
        }
        let document = make_document(3, 7).unwrap();
        assert_eq!(document.graph.nodes[0].kind, NodeKind::Process);
        assert_eq!(document.graph.nodes[1].kind, NodeKind::DataStore);
        assert_eq!(document.graph.nodes[2].kind, NodeKind::ExternalEntity);
        let targets: Vec<_> = document
            .graph
            .edges
            .iter()
            .map(|edge| edge.target.as_str())
            .collect();
        assert_eq!(
            targets,
            [
                "n-0001", "n-0002", "n-0000", "n-0002", "n-0000", "n-0001", "n-0001"
            ]
        );
        assert_eq!(
            make_document(100, 0).unwrap().layout.nodes["n-0010"].x,
            40.0
        );
        assert_eq!(
            make_document(100, 0).unwrap().layout.nodes["n-0010"].y,
            200.0
        );
        assert_eq!(
            make_document(101, 0).unwrap().layout.nodes["n-0010"].x,
            2440.0
        );
        assert_eq!(
            make_document(101, 0).unwrap().layout.nodes["n-0010"].y,
            40.0
        );
    }

    #[test]
    fn fixture_rejects_graphs_too_small_to_connect() {
        assert!(make_document(0, 0).is_err());
        assert!(make_document(1, 0).is_err());
        assert!(make_document(2, 0).is_ok());
    }

    #[test]
    fn grouped_performance_fixtures_cover_every_resource() {
        for (nodes, edges) in [(100, 200), (500, 1000)] {
            let doc = make_grouped_document(nodes, edges).unwrap();
            assert_eq!(doc.graph.groups.len(), nodes / 5);
            assert!(doc.graph.nodes.iter().all(|n| n.group_id.is_some()));
            doc.validate().unwrap();
        }
    }
}
