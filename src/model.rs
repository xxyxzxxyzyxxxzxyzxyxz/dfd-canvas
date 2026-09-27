//! Portable document format. No browser, rendering, or editor state is stored here.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
pub const MIN_NODE_WIDTH: f64 = 80.0;
pub const MIN_NODE_HEIGHT: f64 = 48.0;
pub const MAX_NODE_SIZE: f64 = 10_000.0;
pub const MAX_COORDINATE: f64 = 1_000_000.0;
pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 4.0;
pub const GROUP_PADDING: f64 = 16.0;
pub const GROUP_HEADER_HEIGHT: f64 = 32.0;
pub const MIN_GROUP_WIDTH: f64 = MIN_NODE_WIDTH + GROUP_PADDING * 2.0;
pub const MIN_GROUP_HEIGHT: f64 = MIN_NODE_HEIGHT + GROUP_PADDING * 2.0 + GROUP_HEADER_HEIGHT;
pub const MAX_GROUP_SIZE: f64 = MAX_COORDINATE * 2.0 + MAX_NODE_SIZE + GROUP_PADDING * 2.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub name: String,
    pub graph: Graph,
    pub layout: Layout,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            name: "無題のDFD".into(),
            graph: Graph::default(),
            layout: Layout::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub description: String,
    pub group_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Process,
    DataStore,
    ExternalEntity,
}

impl NodeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Process => "処理",
            Self::DataStore => "データストア",
            Self::ExternalEntity => "外部主体",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub nodes: BTreeMap<String, NodeLayout>,
    pub edges: BTreeMap<String, EdgeLayout>,
    #[serde(default)]
    pub groups: BTreeMap<String, GroupLayout>,
    pub viewport: Viewport,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub type GroupLayout = NodeLayout;

impl NodeLayout {
    /// Small tolerance covers rounding when translating fractional coordinates.
    pub fn contains(self, other: Self) -> bool {
        const EPSILON: f64 = 1e-7;
        other.x >= self.x - EPSILON
            && other.y >= self.y - EPSILON
            && other.x + other.width <= self.x + self.width + EPSILON
            && other.y + other.height <= self.y + self.height + EPSILON
    }

    pub fn intersects(self, other: Self) -> bool {
        const EPSILON: f64 = 1e-7;
        self.x < other.x + other.width - EPSILON
            && other.x < self.x + self.width - EPSILON
            && self.y < other.y + other.height - EPSILON
            && other.y < self.y + self.height - EPSILON
    }

    pub fn group_content(self) -> Self {
        Self {
            x: self.x + GROUP_PADDING,
            y: self.y + GROUP_HEADER_HEIGHT + GROUP_PADDING,
            width: self.width - GROUP_PADDING * 2.0,
            height: self.height - GROUP_HEADER_HEIGHT - GROUP_PADDING * 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeLayout {
    pub source_port: Port,
    pub target_port: Port,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Port {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub pan_x: f64,
    pub pan_y: f64,
    pub zoom: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "未対応の schema_version です: {}（対応: {SCHEMA_VERSION}）",
                self.schema_version
            ));
        }
        let mut all_ids = BTreeSet::new();
        let mut node_ids = BTreeSet::new();
        for node in &self.graph.nodes {
            validate_id(&node.id, &mut all_ids)?;
            node_ids.insert(node.id.as_str());
            let rect = self
                .layout
                .nodes
                .get(&node.id)
                .ok_or_else(|| format!("ノード {} の配置情報がありません", node.id))?;
            if !valid_coordinate(rect.x) || !valid_coordinate(rect.y) {
                return Err(format!("ノード {} の座標が不正です", node.id));
            }
            if !rect.width.is_finite()
                || !rect.height.is_finite()
                || !(MIN_NODE_WIDTH..=MAX_NODE_SIZE).contains(&rect.width)
                || !(MIN_NODE_HEIGHT..=MAX_NODE_SIZE).contains(&rect.height)
            {
                return Err(format!("ノード {} のサイズが不正です", node.id));
            }
        }
        let mut edge_ids = BTreeSet::new();
        for edge in &self.graph.edges {
            validate_id(&edge.id, &mut all_ids)?;
            edge_ids.insert(edge.id.as_str());
            if !node_ids.contains(edge.source.as_str()) || !node_ids.contains(edge.target.as_str())
            {
                return Err(format!("エッジ {} の接続先・接続元が存在しません", edge.id));
            }
            if edge.source == edge.target {
                return Err(format!("エッジ {} は未対応の自己接続です", edge.id));
            }
            if !self.layout.edges.contains_key(&edge.id) {
                return Err(format!("エッジ {} の接続点情報がありません", edge.id));
            }
        }
        if self
            .layout
            .nodes
            .keys()
            .any(|id| !node_ids.contains(id.as_str()))
        {
            return Err("存在しないノードの配置情報があります".into());
        }
        if self
            .layout
            .edges
            .keys()
            .any(|id| !edge_ids.contains(id.as_str()))
        {
            return Err("存在しないエッジの接続点情報があります".into());
        }
        let mut group_ids = BTreeSet::new();
        for group in &self.graph.groups {
            validate_id(&group.id, &mut all_ids)?;
            group_ids.insert(group.id.as_str());
            let rect = self
                .layout
                .groups
                .get(&group.id)
                .ok_or_else(|| format!("グループ {} の配置情報がありません", group.id))?;
            if !valid_coordinate(rect.x)
                || !valid_coordinate(rect.y)
                || !rect.width.is_finite()
                || !rect.height.is_finite()
                || !(MIN_GROUP_WIDTH..=MAX_GROUP_SIZE).contains(&rect.width)
                || !(MIN_GROUP_HEIGHT..=MAX_GROUP_SIZE).contains(&rect.height)
            {
                return Err(format!(
                    "グループ {} の座標またはサイズが不正です",
                    group.id
                ));
            }
        }
        if self
            .layout
            .groups
            .keys()
            .any(|id| !group_ids.contains(id.as_str()))
        {
            return Err("存在しないグループの配置情報があります".into());
        }
        for (index, group) in self.graph.groups.iter().enumerate() {
            let rect = self.layout.groups[&group.id];
            if self.graph.groups[..index]
                .iter()
                .any(|other| rect.intersects(self.layout.groups[&other.id]))
            {
                return Err("グループ同士の枠は重ねられません".into());
            }
        }
        for node in &self.graph.nodes {
            if node
                .group_id
                .as_ref()
                .is_some_and(|id| !group_ids.contains(id.as_str()))
            {
                return Err(format!("ノード {} の所属先グループが存在しません", node.id));
            }
            let actual = self.group_at(self.layout.nodes[&node.id])?;
            if actual != node.group_id {
                return Err(format!(
                    "ノード {} とグループ枠の所属が一致しません。所属外のノードを囲む配置はできません",
                    node.id
                ));
            }
        }
        let view = self.layout.viewport;
        if !valid_coordinate(view.pan_x)
            || !valid_coordinate(view.pan_y)
            || !view.zoom.is_finite()
            || !(MIN_ZOOM..=MAX_ZOOM).contains(&view.zoom)
        {
            return Err("表示位置またはズーム値が不正です".into());
        }
        Ok(())
    }

    /// Whole-node containment determines membership. Header, padding and border
    /// crossings are invalid, even when the node's center is inside the frame.
    pub fn group_at(&self, rect: NodeLayout) -> Result<Option<String>, String> {
        let mut candidate = None;
        for (id, frame) in &self.layout.groups {
            if frame.group_content().contains(rect) {
                if candidate.is_some() {
                    return Err("グループ同士の枠は重ねられません".into());
                }
                candidate = Some(id.clone());
            } else if frame.intersects(rect) {
                return Err("ノードがグループの境界・見出し・余白に重なっています。ノード全体を内容領域の内側か、枠の外側へ移動してください".into());
            }
        }
        Ok(candidate)
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > MAX_FILE_BYTES {
            return Err("ファイルが大きすぎます。読み込み上限は5 MiBです".into());
        }
        let mut document: Self = serde_json::from_str(json).map_err(|error| {
            if error.to_string().contains("unknown variant") {
                format!("未対応のノード種類または接続点です: {error}")
            } else {
                format!("JSONの形式が不正です: {error}")
            }
        })?;
        if document.schema_version == 1 {
            if !document.graph.groups.is_empty()
                || !document.layout.groups.is_empty()
                || document.graph.nodes.iter().any(|n| n.group_id.is_some())
            {
                return Err("v1文書にグループ情報は使用できません".into());
            }
            document.schema_version = SCHEMA_VERSION;
        }
        document.validate()?;
        Ok(document)
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        let mut sorted = self.clone();
        sorted.graph.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        sorted.graph.edges.sort_by(|a, b| a.id.cmp(&b.id));
        sorted.graph.groups.sort_by(|a, b| a.id.cmp(&b.id));
        serde_json::to_string_pretty(&sorted).map_err(|e| format!("JSONを生成できません: {e}"))
    }

    /// Derive semantic JSON without creating a second source of truth.
    pub fn ai_json(&self) -> Result<String, String> {
        #[derive(Serialize)]
        struct SemanticGraph<'a> {
            nodes: Vec<&'a Node>,
            edges: Vec<&'a Edge>,
            groups: Vec<&'a Group>,
        }
        #[derive(Serialize)]
        struct SemanticDocument<'a> {
            schema_version: u32,
            name: &'a str,
            graph: SemanticGraph<'a>,
        }
        self.validate()?;
        let mut nodes: Vec<_> = self.graph.nodes.iter().collect();
        let mut edges: Vec<_> = self.graph.edges.iter().collect();
        let mut groups: Vec<_> = self.graph.groups.iter().collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        serde_json::to_string_pretty(&SemanticDocument {
            schema_version: self.schema_version,
            name: &self.name,
            graph: SemanticGraph {
                nodes,
                edges,
                groups,
            },
        })
        .map_err(|e| format!("AI向けJSONを生成できません: {e}"))
    }
}

fn validate_id<'a>(id: &'a str, seen: &mut BTreeSet<&'a str>) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("空のIDは使用できません".into());
    }
    if !seen.insert(id) {
        return Err(format!("IDが重複しています: {id}"));
    }
    Ok(())
}

pub fn valid_coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_COORDINATE
}
