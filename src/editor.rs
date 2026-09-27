//! User operations and bounded snapshot history, independent of the UI.
use crate::model::{
    Document, Edge, EdgeLayout, GROUP_HEADER_HEIGHT, GROUP_PADDING, Group, MAX_COORDINATE, Node,
    NodeKind, NodeLayout, Port,
};
use std::collections::{BTreeSet, VecDeque};

pub const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeEnd {
    Source,
    Target,
}

pub struct Editor {
    pub document: Document,
    undo: VecDeque<Document>,
    redo: Vec<Document>,
    next_id: u64,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            undo: VecDeque::new(),
            redo: Vec::new(),
            next_id: 1,
        }
    }

    /// Call once on pointerdown / edit start, never on pointermove.
    pub fn begin(&self) -> Document {
        self.document.clone()
    }

    /// Call once when a complete operation is accepted.
    pub fn commit(&mut self, before: Document) -> bool {
        if before == self.document {
            return false;
        }
        self.push_undo(before);
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(document) = self.undo.pop_back() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.document, document));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(document) = self.redo.pop() else {
            return false;
        };
        let before = std::mem::replace(&mut self.document, document);
        self.push_undo(before);
        true
    }

    pub fn replace(&mut self, document: Document) {
        self.document = document;
        self.undo.clear();
        self.redo.clear();
    }

    /// Parse and validate fully before changing the current document.
    pub fn load_json(&mut self, json: &str) -> Result<(), String> {
        let document = Document::from_json(json)?;
        self.replace(document);
        Ok(())
    }

    pub fn add_node(&mut self, kind: NodeKind, x: f64, y: f64) -> Result<String, String> {
        let rect = NodeLayout {
            x: bounded_coordinate(x),
            y: bounded_coordinate(y),
            width: 160.0,
            height: 80.0,
        };
        let group_id = self.document.group_at(rect)?;
        let before = self.begin();
        let id = self.allocate_id("node");
        self.document.graph.nodes.push(Node {
            id: id.clone(),
            kind,
            name: kind.label().into(),
            description: String::new(),
            group_id,
        });
        self.document.layout.nodes.insert(id.clone(), rect);
        self.commit(before);
        Ok(id)
    }

    pub fn create_group(&mut self, selected: &BTreeSet<String>) -> Result<String, String> {
        let nodes: Vec<_> = self
            .document
            .graph
            .nodes
            .iter()
            .filter(|n| selected.contains(&n.id))
            .collect();
        if nodes.is_empty() {
            return Err("グループにするノードを選択してください".into());
        }
        if nodes.iter().any(|n| n.group_id.is_some()) {
            return Err(
                "すでに所属しているノードは、枠の外へ移動してからグループを作成してください".into(),
            );
        }
        let rects: Vec<_> = nodes
            .iter()
            .map(|n| self.document.layout.nodes[&n.id])
            .collect();
        let left = rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min) - GROUP_PADDING;
        let top = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min)
            - GROUP_PADDING
            - GROUP_HEADER_HEIGHT;
        let right = rects
            .iter()
            .map(|r| r.x + r.width)
            .fold(f64::NEG_INFINITY, f64::max)
            + GROUP_PADDING;
        let bottom = rects
            .iter()
            .map(|r| r.y + r.height)
            .fold(f64::NEG_INFINITY, f64::max)
            + GROUP_PADDING;
        let before = self.begin();
        let id = self.allocate_id("group");
        self.document.graph.groups.push(Group {
            id: id.clone(),
            name: "グループ".into(),
            description: String::new(),
        });
        self.document.layout.groups.insert(
            id.clone(),
            NodeLayout {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            },
        );
        for node in &mut self.document.graph.nodes {
            if selected.contains(&node.id) {
                node.group_id = Some(id.clone());
            }
        }
        self.finish_layout(before, &BTreeSet::new())?;
        Ok(id)
    }

    /// Finalize a gesture atomically. Only explicitly moved nodes may change
    /// membership; moving/resizing a frame never captures unrelated nodes.
    pub fn finish_layout(
        &mut self,
        before: Document,
        moved_nodes: &BTreeSet<String>,
    ) -> Result<bool, String> {
        let result = (|| {
            let memberships: Result<Vec<_>, String> = self
                .document
                .graph
                .nodes
                .iter()
                .filter(|n| moved_nodes.contains(&n.id))
                .map(|n| {
                    self.document
                        .group_at(self.document.layout.nodes[&n.id])
                        .map(|group| (n.id.clone(), group))
                })
                .collect();
            for (id, group) in memberships? {
                self.document
                    .graph
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == id)
                    .unwrap()
                    .group_id = group;
            }
            self.document.validate()
        })();
        if let Err(error) = result {
            self.document = before;
            return Err(error);
        }
        Ok(self.commit(before))
    }

    pub fn add_edge(
        &mut self,
        source: &str,
        target: &str,
        source_port: Port,
        target_port: Port,
    ) -> Result<String, String> {
        if source == target {
            return Err("自分自身への接続はできません。別のノードを選んでください".into());
        }
        if !self.document.graph.nodes.iter().any(|n| n.id == source)
            || !self.document.graph.nodes.iter().any(|n| n.id == target)
        {
            return Err("接続元または接続先のノードが存在しません".into());
        }
        let before = self.begin();
        let id = self.allocate_id("edge");
        self.document.graph.edges.push(Edge {
            id: id.clone(),
            source: source.into(),
            target: target.into(),
            name: "データ".into(),
            description: String::new(),
        });
        self.document.layout.edges.insert(
            id.clone(),
            EdgeLayout {
                source_port,
                target_port,
            },
        );
        self.commit(before);
        Ok(id)
    }

    /// Move an edge endpoint to another port on its existing node.
    pub fn set_edge_port(&mut self, id: &str, end: EdgeEnd, port: Port) -> Result<bool, String> {
        if !self.document.graph.edges.iter().any(|edge| edge.id == id) {
            return Err("接続線が存在しません".into());
        }
        let mut layout = *self
            .document
            .layout
            .edges
            .get(id)
            .ok_or("接続線の配置情報が存在しません")?;
        let current = match end {
            EdgeEnd::Source => &mut layout.source_port,
            EdgeEnd::Target => &mut layout.target_port,
        };
        if *current == port {
            return Ok(false);
        }
        *current = port;
        let before = self.begin();
        self.document.layout.edges.insert(id.into(), layout);
        Ok(self.commit(before))
    }

    pub fn delete(&mut self, selected: &BTreeSet<String>) -> bool {
        if selected.is_empty() {
            return false;
        }
        let before = self.begin();
        let removed_edges: BTreeSet<_> = self
            .document
            .graph
            .edges
            .iter()
            .filter(|edge| {
                selected.contains(&edge.id)
                    || selected.contains(&edge.source)
                    || selected.contains(&edge.target)
            })
            .map(|edge| edge.id.clone())
            .collect();
        self.document
            .graph
            .nodes
            .retain(|node| !selected.contains(&node.id));
        self.document
            .layout
            .nodes
            .retain(|id, _| !selected.contains(id));
        self.document
            .graph
            .edges
            .retain(|edge| !removed_edges.contains(&edge.id));
        self.document
            .layout
            .edges
            .retain(|id, _| !removed_edges.contains(id));
        self.document
            .graph
            .groups
            .retain(|group| !selected.contains(&group.id));
        self.document
            .layout
            .groups
            .retain(|id, _| !selected.contains(id));
        for node in &mut self.document.graph.nodes {
            if node
                .group_id
                .as_ref()
                .is_some_and(|id| selected.contains(id))
            {
                node.group_id = None;
            }
        }
        self.commit(before)
    }

    fn push_undo(&mut self, document: Document) {
        self.undo.push_back(document);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.pop_front();
        }
    }

    fn allocate_id(&mut self, prefix: &str) -> String {
        loop {
            let id = format!("{prefix}-{:06}", self.next_id);
            self.next_id += 1;
            if !self.document.graph.nodes.iter().any(|node| node.id == id)
                && !self.document.graph.edges.iter().any(|edge| edge.id == id)
                && !self
                    .document
                    .graph
                    .groups
                    .iter()
                    .any(|group| group.id == id)
            {
                return id;
            }
        }
    }
}

fn bounded_coordinate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-MAX_COORDINATE, MAX_COORDINATE)
    } else {
        0.0
    }
}
