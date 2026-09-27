use crate::storage;
use dfd_canvas::{
    editor::{EdgeEnd, Editor},
    geometry::*,
    model::*,
};
use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{KeyboardEvent, PointerEvent};

#[derive(Clone, Copy)]
struct NodeVm {
    meta: RwSignal<Node>,
    rect: RwSignal<NodeLayout>,
}
#[derive(Clone, Copy)]
struct GroupVm {
    meta: RwSignal<Group>,
    rect: RwSignal<GroupLayout>,
}
#[derive(Clone, Copy)]
struct EdgeVm {
    meta: RwSignal<Edge>,
    ports: RwSignal<EdgeLayout>,
    offset: RwSignal<f64>,
    source: RwSignal<NodeLayout>,
    target: RwSignal<NodeLayout>,
}
#[derive(Clone)]
struct EdgePreview {
    id: String,
    end: EdgeEnd,
    point: Point,
    port: Port,
}
enum Drag {
    Move {
        before: Document,
        start: Point,
        nodes: Vec<(String, NodeLayout, RwSignal<NodeLayout>)>,
        groups: Vec<(String, GroupLayout, RwSignal<GroupLayout>)>,
        reassign: BTreeSet<String>,
    },
    Resize {
        before: Document,
        start: Point,
        id: String,
        rect: NodeLayout,
        signal: RwSignal<NodeLayout>,
        group: bool,
        min_width: f64,
        min_height: f64,
    },
    Pan {
        before: Document,
        start: Point,
        viewport: Viewport,
    },
    Connect {
        source: String,
        port: Port,
        start: Point,
    },
    Reconnect {
        id: String,
        end: EdgeEnd,
        node: String,
        port: Port,
    },
}
struct Runtime {
    editor: RefCell<Editor>,
    drag: RefCell<Option<Drag>>,
    timer: RefCell<Option<Timeout>>,
    blocked_storage: bool,
}
#[derive(Clone, Copy)]
struct Ui {
    runtime: StoredValue<Runtime, LocalStorage>,
    nodes: RwSignal<Vec<NodeVm>>,
    edges: RwSignal<Vec<EdgeVm>>,
    groups: RwSignal<Vec<GroupVm>>,
    selection: RwSignal<BTreeSet<String>>,
    viewport: RwSignal<Viewport>,
    revision: RwSignal<u64>,
    title: RwSignal<String>,
    error: RwSignal<String>,
    status: RwSignal<String>,
    undo: RwSignal<bool>,
    redo: RwSignal<bool>,
    dirty: RwSignal<bool>,
    autosave_pending: RwSignal<bool>,
    draft: RwSignal<Option<(Point, Port, Point)>>,
    edge_preview: RwSignal<Option<EdgePreview>>,
    candidate: RwSignal<Option<(String, Port)>>,
    group_candidates: RwSignal<BTreeSet<String>>,
    invalid_placement: RwSignal<bool>,
    ai_open: RwSignal<bool>,
    ai_text: RwSignal<String>,
}
impl Ui {
    fn new() -> Self {
        let (doc, blocked, error, status) = match storage::restore() {
            Ok(Some(doc)) => (
                doc,
                false,
                String::new(),
                "ブラウザ内の自動保存から復元しました".into(),
            ),
            Ok(None) => (
                Document::default(),
                false,
                String::new(),
                "編集後にブラウザ内へ自動保存します".into(),
            ),
            Err(e) => (
                Document::default(),
                true,
                format!(
                    "復旧データを利用できません: {e}。既存データを保護するため、このセッションの自動保存を停止しています。JSONファイルで保存してください。"
                ),
                "自動保存停止・既存データを保護中".into(),
            ),
        };
        let ui = Self {
            viewport: RwSignal::new(doc.layout.viewport),
            title: RwSignal::new(doc.name.clone()),
            runtime: StoredValue::new_local(Runtime {
                editor: RefCell::new(Editor::new(doc)),
                drag: RefCell::new(None),
                timer: RefCell::new(None),
                blocked_storage: blocked,
            }),
            nodes: RwSignal::new(Vec::new()),
            edges: RwSignal::new(Vec::new()),
            groups: RwSignal::new(Vec::new()),
            selection: RwSignal::new(BTreeSet::new()),
            revision: RwSignal::new(0),
            error: RwSignal::new(error),
            status: RwSignal::new(status),
            undo: RwSignal::new(false),
            redo: RwSignal::new(false),
            dirty: RwSignal::new(false),
            autosave_pending: RwSignal::new(false),
            draft: RwSignal::new(None),
            edge_preview: RwSignal::new(None),
            candidate: RwSignal::new(None),
            group_candidates: RwSignal::new(BTreeSet::new()),
            invalid_placement: RwSignal::new(false),
            ai_open: RwSignal::new(false),
            ai_text: RwSignal::new(String::new()),
        };
        ui.sync();
        ui
    }
    // Synchronize projections only at operation boundaries. Existing keyed DOM is retained.
    fn sync(self) {
        self.runtime.with_value(|rt| {
            let editor = rt.editor.borrow();
            let doc = &editor.document;
            let old_groups: BTreeMap<_, _> = self
                .groups
                .get_untracked()
                .into_iter()
                .map(|vm| (vm.meta.with_untracked(|g| g.id.clone()), vm))
                .collect();
            let groups = doc
                .graph
                .groups
                .iter()
                .map(|g| {
                    if let Some(vm) = old_groups.get(&g.id) {
                        if vm.meta.with_untracked(|old| old != g) {
                            vm.meta.set(g.clone());
                        }
                        if vm.rect.get_untracked() != doc.layout.groups[&g.id] {
                            vm.rect.set(doc.layout.groups[&g.id]);
                        }
                        *vm
                    } else {
                        GroupVm {
                            meta: RwSignal::new(g.clone()),
                            rect: RwSignal::new(doc.layout.groups[&g.id]),
                        }
                    }
                })
                .collect();
            let old_nodes: BTreeMap<_, _> = self
                .nodes
                .get_untracked()
                .into_iter()
                .map(|vm| (vm.meta.with_untracked(|n| n.id.clone()), vm))
                .collect();
            let nodes: Vec<_> = doc
                .graph
                .nodes
                .iter()
                .map(|n| {
                    if let Some(vm) = old_nodes.get(&n.id) {
                        if vm.meta.with_untracked(|old| old != n) {
                            vm.meta.set(n.clone());
                        }
                        if vm.rect.get_untracked() != doc.layout.nodes[&n.id] {
                            vm.rect.set(doc.layout.nodes[&n.id]);
                        }
                        *vm
                    } else {
                        NodeVm {
                            meta: RwSignal::new(n.clone()),
                            rect: RwSignal::new(doc.layout.nodes[&n.id]),
                        }
                    }
                })
                .collect();
            let by_id: BTreeMap<_, _> = nodes
                .iter()
                .map(|vm| (vm.meta.with_untracked(|n| n.id.clone()), vm.rect))
                .collect();
            let old_edges: BTreeMap<_, _> = self
                .edges
                .get_untracked()
                .into_iter()
                .map(|vm| (vm.meta.with_untracked(|e| e.id.clone()), vm))
                .collect();
            let mut edge_pairs: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
            for e in &doc.graph.edges {
                let pair = if e.source < e.target {
                    (e.source.clone(), e.target.clone())
                } else {
                    (e.target.clone(), e.source.clone())
                };
                edge_pairs.entry(pair).or_default().push(e.id.clone());
            }
            let mut offsets = BTreeMap::new();
            for ids in edge_pairs.values_mut() {
                ids.sort();
                for (i, id) in ids.iter().enumerate() {
                    offsets.insert(id.clone(), (i as f64 - (ids.len() - 1) as f64 / 2.0) * 44.0);
                }
            }
            let edges =
                doc.graph
                    .edges
                    .iter()
                    .map(|e| {
                        let offset = offsets[&e.id] * if e.source < e.target { 1.0 } else { -1.0 };
                        if let Some(vm) = old_edges.get(&e.id).filter(|vm| {
                            vm.source == by_id[&e.source] && vm.target == by_id[&e.target]
                        }) {
                            if vm.meta.with_untracked(|old| old != e) {
                                vm.meta.set(e.clone());
                            }
                            vm.ports.set(doc.layout.edges[&e.id]);
                            vm.offset.set(offset);
                            *vm
                        } else {
                            EdgeVm {
                                meta: RwSignal::new(e.clone()),
                                ports: RwSignal::new(doc.layout.edges[&e.id]),
                                offset: RwSignal::new(offset),
                                source: by_id[&e.source],
                                target: by_id[&e.target],
                            }
                        }
                    })
                    .collect();
            self.nodes.set(nodes);
            self.edges.set(edges);
            self.groups.set(groups);
            self.viewport.set(doc.layout.viewport);
            self.title.set(doc.name.clone());
            self.undo.set(editor.can_undo());
            self.redo.set(editor.can_redo());
            self.selection.update(|s| {
                s.retain(|id| {
                    doc.layout.nodes.contains_key(id)
                        || doc.layout.edges.contains_key(id)
                        || doc.layout.groups.contains_key(id)
                })
            });
        });
        self.revision.update(|r| *r += 1);
        if self.ai_open.get_untracked() {
            self.refresh_ai();
        }
    }
    fn committed(self) {
        self.sync();
        self.dirty.set(true);
        self.schedule_view_save();
    }
    fn select(self, id: &str, shift: bool) {
        self.selection.update(|s| {
            if shift {
                if !s.remove(id) {
                    s.insert(id.into());
                }
            } else if !s.contains(id) {
                s.clear();
                s.insert(id.into());
            }
        });
    }
    fn add(self, kind: NodeKind) {
        self.cancel();
        let size = canvas_rect();
        let count = self.nodes.with_untracked(|n| n.len());
        let p = screen_to_world(
            Point {
                x: size.width() / 2.0 - 80.0 + (count % 3) as f64 * 30.0,
                y: size.height() / 2.0 - 40.0 + (count % 3) as f64 * 30.0,
            },
            self.viewport.get_untracked(),
        );
        let result = self.runtime.with_value(|rt| {
            rt.editor.borrow_mut().add_node(
                kind,
                p.x.clamp(-MAX_COORDINATE, MAX_COORDINATE),
                p.y.clamp(-MAX_COORDINATE, MAX_COORDINATE),
            )
        });
        match result {
            Ok(id) => {
                self.error.set(String::new());
                self.selection.set(BTreeSet::from([id]));
                self.committed();
            }
            Err(e) => self.error.set(e),
        }
    }
    fn create_group(self) {
        self.cancel();
        match self.runtime.with_value(|rt| {
            rt.editor
                .borrow_mut()
                .create_group(&self.selection.get_untracked())
        }) {
            Ok(id) => {
                self.error.set(String::new());
                self.selection.set(BTreeSet::from([id]));
                self.committed();
            }
            Err(e) => self.error.set(e),
        }
    }
    fn delete(self) {
        self.cancel();
        if self.runtime.with_value(|rt| {
            rt.editor
                .borrow_mut()
                .delete(&self.selection.get_untracked())
        }) {
            self.selection.set(BTreeSet::new());
            self.committed();
        }
    }
    fn history(self, redo: bool) {
        self.cancel();
        let changed = self.runtime.with_value(|rt| {
            let mut e = rt.editor.borrow_mut();
            if redo { e.redo() } else { e.undo() }
        });
        if changed {
            self.committed();
        }
    }
    fn refresh_ai(self) {
        match self
            .runtime
            .with_value(|rt| rt.editor.borrow().document.ai_json())
        {
            Ok(t) => self.ai_text.set(t),
            Err(e) => self.error.set(e),
        }
    }
    fn save(self, ai: bool) {
        let result = self.runtime.with_value(|rt| {
            let e = rt.editor.borrow();
            let text = if ai {
                e.document.ai_json()
            } else {
                e.document.to_json()
            }?;
            storage::download(
                &text,
                if ai {
                    "diagram.ai.json"
                } else {
                    "diagram.dfd.json"
                },
            )
        });
        match result {
            Ok(()) => {
                if !ai {
                    self.dirty.set(false);
                }
            }
            Err(e) => self.error.set(e),
        }
    }
    fn replace(self, doc: Document) {
        self.cancel();
        self.runtime
            .with_value(|rt| rt.editor.borrow_mut().replace(doc));
        self.nodes.set(Vec::new());
        self.edges.set(Vec::new());
        self.groups.set(Vec::new());
        self.selection.set(BTreeSet::new());
        self.error.set(String::new());
        self.committed();
        self.dirty.set(false);
    }
    fn may_replace(self) -> bool {
        let nonempty = self.runtime.with_value(|rt| {
            let e = rt.editor.borrow();
            !e.document.graph.nodes.is_empty() || !e.document.graph.groups.is_empty()
        });
        !(self.dirty.get_untracked() || nonempty)
            || web_sys::window()
                .unwrap()
                .confirm_with_message(
                    "現在の文書を置き換えます。必要な内容はJSONファイルへ保存しましたか？",
                )
                .unwrap_or(false)
    }
    fn property(self, id: Option<String>, description: bool, value: String) {
        let changed = self.runtime.with_value(|rt| {
            let mut e = rt.editor.borrow_mut();
            let before = e.begin();
            if let Some(id) = id {
                if let Some(n) = e.document.graph.nodes.iter_mut().find(|n| n.id == id) {
                    if description {
                        n.description = value;
                    } else {
                        n.name = value;
                    }
                } else if let Some(edge) = e.document.graph.edges.iter_mut().find(|n| n.id == id) {
                    if description {
                        edge.description = value;
                    } else {
                        edge.name = value;
                    }
                } else if let Some(group) = e.document.graph.groups.iter_mut().find(|n| n.id == id)
                {
                    if description {
                        group.description = value;
                    } else {
                        group.name = value;
                    }
                }
            } else {
                e.document.name = value;
            }
            e.commit(before)
        });
        if changed {
            self.committed();
        }
    }
    fn capture(self, event: &PointerEvent) {
        let _ = canvas().unchecked_into::<web_sys::HtmlElement>().focus();
        self.runtime.with_value(|rt| {
            rt.timer.borrow_mut().take();
        });
        let _ = canvas().set_pointer_capture(event.pointer_id());
        event.prevent_default();
    }
    fn start_move(self, event: PointerEvent, vm: NodeVm, resize: bool) {
        if event.button() != 0 {
            return;
        }
        event.stop_propagation();
        let id = vm.meta.with_untracked(|n| n.id.clone());
        self.select(&id, event.shift_key());
        if event.shift_key() || !self.selection.with_untracked(|s| s.contains(&id)) {
            return;
        }
        if !resize {
            self.start_selection_move(event);
            return;
        }
        self.capture(&event);
        let start = world_point(&event, self.viewport.get_untracked());
        self.runtime.with_value(|rt| {
            let before = rt.editor.borrow().begin();
            let drag = Drag::Resize {
                before,
                start,
                id,
                rect: vm.rect.get_untracked(),
                signal: vm.rect,
                group: false,
                min_width: MIN_NODE_WIDTH,
                min_height: MIN_NODE_HEIGHT,
            };
            *rt.drag.borrow_mut() = Some(drag);
        });
    }
    fn start_selection_move(self, event: PointerEvent) {
        self.capture(&event);
        let selected = self.selection.get_untracked();
        let mut reassign = BTreeSet::new();
        let nodes = self
            .nodes
            .get_untracked()
            .into_iter()
            .filter_map(|vm| {
                let n = vm.meta.get_untracked();
                let moving_group = n.group_id.as_ref().is_some_and(|id| selected.contains(id));
                if selected.contains(&n.id) || moving_group {
                    if !moving_group {
                        reassign.insert(n.id.clone());
                    }
                    Some((n.id, vm.rect.get_untracked(), vm.rect))
                } else {
                    None
                }
            })
            .collect();
        let groups = self
            .groups
            .get_untracked()
            .into_iter()
            .filter_map(|vm| {
                let id = vm.meta.with_untracked(|g| g.id.clone());
                selected
                    .contains(&id)
                    .then(|| (id, vm.rect.get_untracked(), vm.rect))
            })
            .collect();
        self.runtime.with_value(|rt| {
            *rt.drag.borrow_mut() = Some(Drag::Move {
                before: rt.editor.borrow().begin(),
                start: world_point(&event, self.viewport.get_untracked()),
                nodes,
                groups,
                reassign,
            });
        });
    }
    fn start_group(self, event: PointerEvent, vm: GroupVm, resize: bool) {
        if event.button() != 0 {
            return;
        }
        event.stop_propagation();
        let id = vm.meta.with_untracked(|g| g.id.clone());
        self.select(&id, event.shift_key());
        if event.shift_key() {
            return;
        }
        if !resize {
            self.start_selection_move(event);
            return;
        }
        self.capture(&event);
        let rect = vm.rect.get_untracked();
        self.runtime.with_value(|rt| {
            let e = rt.editor.borrow();
            let mut min_width = MIN_GROUP_WIDTH;
            let mut min_height = MIN_GROUP_HEIGHT;
            for n in e
                .document
                .graph
                .nodes
                .iter()
                .filter(|n| n.group_id.as_ref() == Some(&id))
            {
                let r = e.document.layout.nodes[&n.id];
                min_width = min_width.max(r.x + r.width + GROUP_PADDING - rect.x);
                min_height = min_height.max(r.y + r.height + GROUP_PADDING - rect.y);
            }
            *rt.drag.borrow_mut() = Some(Drag::Resize {
                before: e.begin(),
                start: world_point(&event, self.viewport.get_untracked()),
                id,
                rect,
                signal: vm.rect,
                group: true,
                min_width,
                min_height,
            });
        });
    }
    fn start_connect(self, event: PointerEvent, vm: NodeVm, port: Port) {
        if event.button() != 0 {
            return;
        }
        event.stop_propagation();
        self.capture(&event);
        self.error.set(String::new());
        let source = vm.meta.with_untracked(|n| n.id.clone());
        let start = port_position(vm.rect.get_untracked(), port);
        self.draft.set(Some((start, port, start)));
        self.runtime.with_value(|rt| {
            *rt.drag.borrow_mut() = Some(Drag::Connect {
                source,
                port,
                start,
            })
        });
    }
    fn start_reconnect(self, event: PointerEvent, vm: EdgeVm, end: EdgeEnd) {
        if event.button() != 0 {
            return;
        }
        event.stop_propagation();
        self.cancel();
        self.capture(&event);
        self.error.set(String::new());
        let edge = vm.meta.get_untracked();
        let ports = vm.ports.get_untracked();
        let (node, rect, port) = match end {
            EdgeEnd::Source => (edge.source, vm.source.get_untracked(), ports.source_port),
            EdgeEnd::Target => (edge.target, vm.target.get_untracked(), ports.target_port),
        };
        self.selection.set(BTreeSet::from([edge.id.clone()]));
        self.edge_preview.set(Some(EdgePreview {
            id: edge.id.clone(),
            end,
            point: port_position(rect, port),
            port,
        }));
        self.runtime.with_value(|rt| {
            *rt.drag.borrow_mut() = Some(Drag::Reconnect {
                id: edge.id,
                end,
                node,
                port,
            });
        });
    }
    fn start_pan(self, event: PointerEvent) {
        if event.button() != 0 && event.button() != 1 {
            return;
        }
        self.capture(&event);
        if !event.shift_key() {
            self.selection.set(BTreeSet::new());
        }
        self.runtime.with_value(|rt| {
            *rt.drag.borrow_mut() = Some(Drag::Pan {
                before: rt.editor.borrow().begin(),
                start: client_point(&event),
                viewport: self.viewport.get_untracked(),
            })
        });
    }
    fn hit_port(self, p: Point) -> Option<(String, Port)> {
        let radius = 12.0 / self.viewport.get_untracked().zoom;
        self.nodes.with_untracked(|nodes| {
            nodes.iter().rev().find_map(|vm| {
                [Port::Top, Port::Right, Port::Bottom, Port::Left]
                    .into_iter()
                    .find_map(|port| {
                        let q = port_position(vm.rect.get_untracked(), port);
                        ((p.x - q.x).hypot(p.y - q.y) <= radius)
                            .then(|| (vm.meta.with_untracked(|n| n.id.clone()), port))
                    })
            })
        })
    }
    fn hit_node_port(self, p: Point, node: &str) -> Option<Port> {
        let radius = 12.0 / self.viewport.get_untracked().zoom;
        self.nodes.with_untracked(|nodes| {
            let vm = nodes
                .iter()
                .find(|vm| vm.meta.with_untracked(|n| n.id == node))?;
            [Port::Top, Port::Right, Port::Bottom, Port::Left]
                .into_iter()
                .filter_map(|port| {
                    let q = port_position(vm.rect.get_untracked(), port);
                    let distance = (p.x - q.x).hypot(p.y - q.y);
                    (distance <= radius).then_some((port, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(port, _)| port)
        })
    }
    fn pointer_move(self, event: PointerEvent) {
        let world = world_point(&event, self.viewport.get_untracked());
        self.runtime.with_value(|rt| {
            let drag = rt.drag.borrow();
            let Some(drag) = drag.as_ref() else {
                return;
            };
            match drag {
                Drag::Move {
                    start,
                    nodes,
                    groups,
                    reassign,
                    ..
                } => {
                    let mut e = rt.editor.borrow_mut();
                    // Clamp the shared delta, preserving every relative position.
                    let mut dx = world.x - start.x;
                    let mut dy = world.y - start.y;
                    for (_, r, _) in nodes.iter().chain(groups.iter()) {
                        dx = dx.clamp(-MAX_COORDINATE - r.x, MAX_COORDINATE - r.x);
                        dy = dy.clamp(-MAX_COORDINATE - r.y, MAX_COORDINATE - r.y);
                    }
                    for (id, original, signal) in nodes {
                        let mut r = *original;
                        r.x += dx;
                        r.y += dy;
                        e.document.layout.nodes.insert(id.clone(), r);
                        signal.set(r);
                    }
                    for (id, original, signal) in groups {
                        let mut r = *original;
                        r.x += dx;
                        r.y += dy;
                        e.document.layout.groups.insert(id.clone(), r);
                        signal.set(r);
                    }
                    let mut candidates = BTreeSet::new();
                    let mut invalid = false;
                    for id in reassign {
                        match e.document.group_at(e.document.layout.nodes[id]) {
                            Ok(Some(group)) => {
                                candidates.insert(group);
                            }
                            Ok(None) => (),
                            Err(_) => invalid = true,
                        }
                    }
                    // Frame gestures keep membership fixed. A full check also
                    // catches overlaps and accidental enclosure of other nodes.
                    if !groups.is_empty() && reassign.is_empty() {
                        invalid |= e.document.validate().is_err();
                    }
                    if self.group_candidates.get_untracked() != candidates {
                        self.group_candidates.set(candidates);
                    }
                    if self.invalid_placement.get_untracked() != invalid {
                        self.invalid_placement.set(invalid);
                    }
                }
                Drag::Resize {
                    start,
                    id,
                    rect,
                    signal,
                    group,
                    min_width,
                    min_height,
                    ..
                } => {
                    let mut r = *rect;
                    let max = if *group {
                        MAX_GROUP_SIZE
                    } else {
                        MAX_NODE_SIZE
                    };
                    r.width = (rect.width + world.x - start.x).clamp(*min_width, max);
                    r.height = (rect.height + world.y - start.y).clamp(*min_height, max);
                    let mut e = rt.editor.borrow_mut();
                    if *group {
                        e.document.layout.groups.insert(id.clone(), r);
                    } else {
                        e.document.layout.nodes.insert(id.clone(), r);
                    }
                    let invalid = e.document.validate().is_err();
                    if self.invalid_placement.get_untracked() != invalid {
                        self.invalid_placement.set(invalid);
                    }
                    signal.set(r);
                }
                Drag::Pan {
                    start, viewport, ..
                } => {
                    let p = client_point(&event);
                    let mut v = *viewport;
                    v.pan_x =
                        (viewport.pan_x + p.x - start.x).clamp(-MAX_COORDINATE, MAX_COORDINATE);
                    v.pan_y =
                        (viewport.pan_y + p.y - start.y).clamp(-MAX_COORDINATE, MAX_COORDINATE);
                    rt.editor.borrow_mut().document.layout.viewport = v;
                    self.viewport.set(v);
                }
                Drag::Connect {
                    source,
                    port,
                    start,
                } => {
                    let hit = self.hit_port(world);
                    let end = hit
                        .as_ref()
                        .and_then(|(id, port)| {
                            rt.editor
                                .borrow()
                                .document
                                .layout
                                .nodes
                                .get(id)
                                .map(|r| port_position(*r, *port))
                        })
                        .unwrap_or(world);
                    self.candidate.set(hit.filter(|(id, _)| id != source));
                    self.draft.set(Some((*start, *port, end)));
                }
                Drag::Reconnect {
                    id,
                    end,
                    node,
                    port,
                } => {
                    let hit = self.hit_node_port(world, node);
                    let point = hit
                        .map(|p| port_position(rt.editor.borrow().document.layout.nodes[node], p))
                        .unwrap_or(world);
                    self.candidate.set(hit.map(|p| (node.clone(), p)));
                    self.edge_preview.set(Some(EdgePreview {
                        id: id.clone(),
                        end: *end,
                        point,
                        port: hit.unwrap_or(*port),
                    }));
                }
            }
        });
    }
    fn finish(self, event: PointerEvent) {
        self.pointer_move(event.clone());
        let drag = self.runtime.with_value(|rt| rt.drag.borrow_mut().take());
        let changed = match drag {
            Some(Drag::Move {
                before, reassign, ..
            }) => self.finish_layout(before, reassign),
            Some(Drag::Resize { before, .. }) => self.finish_layout(before, BTreeSet::new()),
            Some(Drag::Pan { before, .. }) => self
                .runtime
                .with_value(|rt| rt.editor.borrow_mut().commit(before)),
            Some(Drag::Connect { source, port, .. }) => {
                if let Some((target, target_port)) =
                    self.hit_port(world_point(&event, self.viewport.get_untracked()))
                {
                    match self.runtime.with_value(|rt| {
                        rt.editor
                            .borrow_mut()
                            .add_edge(&source, &target, port, target_port)
                    }) {
                        Ok(id) => {
                            self.selection.set(BTreeSet::from([id]));
                            true
                        }
                        Err(e) => {
                            self.error.set(e);
                            false
                        }
                    }
                } else {
                    false
                }
            }
            Some(Drag::Reconnect { id, end, node, .. }) => {
                if let Some(port) =
                    self.hit_node_port(world_point(&event, self.viewport.get_untracked()), &node)
                {
                    match self
                        .runtime
                        .with_value(|rt| rt.editor.borrow_mut().set_edge_port(&id, end, port))
                    {
                        Ok(changed) => changed,
                        Err(error) => {
                            self.error.set(error);
                            false
                        }
                    }
                } else {
                    false
                }
            }
            None => false,
        };
        self.draft.set(None);
        self.edge_preview.set(None);
        self.candidate.set(None);
        self.group_candidates.set(BTreeSet::new());
        self.invalid_placement.set(false);
        let _ = canvas().release_pointer_capture(event.pointer_id());
        if changed {
            self.committed();
        } else if self.autosave_pending.get_untracked() {
            self.schedule_view_save();
        }
    }
    fn finish_layout(self, before: Document, reassign: BTreeSet<String>) -> bool {
        match self
            .runtime
            .with_value(|rt| rt.editor.borrow_mut().finish_layout(before, &reassign))
        {
            Ok(changed) => {
                self.error.set(String::new());
                changed
            }
            Err(error) => {
                self.sync();
                self.error.set(format!("配置を取り消しました: {error}"));
                false
            }
        }
    }
    fn cancel(self) {
        let drag = self.runtime.with_value(|rt| rt.drag.borrow_mut().take());
        if let Some(
            Drag::Move { before, .. } | Drag::Resize { before, .. } | Drag::Pan { before, .. },
        ) = drag
        {
            self.runtime
                .with_value(|rt| rt.editor.borrow_mut().document = before);
            self.sync();
        }
        self.draft.set(None);
        self.edge_preview.set(None);
        self.candidate.set(None);
        self.group_candidates.set(BTreeSet::new());
        self.invalid_placement.set(false);
        if self.autosave_pending.get_untracked() {
            self.schedule_view_save();
        }
    }
    fn zoom(self, factor: f64, anchor: Point) {
        if self.runtime.with_value(|rt| rt.drag.borrow().is_some()) {
            return;
        }
        let v = self.viewport.get_untracked();
        let p = screen_to_world(anchor, v);
        let zoom = (v.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let next = Viewport {
            zoom,
            pan_x: (anchor.x - p.x * zoom).clamp(-MAX_COORDINATE, MAX_COORDINATE),
            pan_y: (anchor.y - p.y * zoom).clamp(-MAX_COORDINATE, MAX_COORDINATE),
        };
        // Wheel updates only the transform; debounce serialization, with no document snapshot per wheel tick.
        self.runtime
            .with_value(|rt| rt.editor.borrow_mut().document.layout.viewport = next);
        self.viewport.set(next);
        self.dirty.set(true);
        self.schedule_view_save();
    }
    fn schedule_view_save(self) {
        self.runtime.with_value(|rt| {
            if rt.blocked_storage {
                return;
            }
            self.autosave_pending.set(true);
            self.status.set("ブラウザ内へ自動保存待ち…".into());
            *rt.timer.borrow_mut() = Some(Timeout::new(550, move || {
                self.autosave_pending.set(false);
                self.runtime.with_value(|rt| {
                    match storage::autosave(&rt.editor.borrow().document) {
                        Ok(()) => self.status.set("ブラウザ内に自動保存済み（復旧用）".into()),
                        Err(e) => self.status.set(e),
                    }
                })
            }));
        });
    }
    fn fit(self) {
        self.cancel();
        let r = canvas_rect();
        let mut bounds: Vec<_> = self.runtime.with_value(|rt| {
            let e = rt.editor.borrow();
            e.document
                .layout
                .nodes
                .values()
                .chain(e.document.layout.groups.values())
                .copied()
                .collect()
        });
        self.edges.with_untracked(|edges| {
            bounds.extend(edges.iter().map(|edge| {
                let ports = edge.ports.get_untracked();
                bezier_bounds(
                    port_position(edge.source.get_untracked(), ports.source_port),
                    ports.source_port,
                    port_position(edge.target.get_untracked(), ports.target_port),
                    ports.target_port,
                    edge.offset.get_untracked(),
                )
            }));
        });
        let v = fit_view(bounds, r.width(), r.height());
        self.runtime
            .with_value(|rt| rt.editor.borrow_mut().document.layout.viewport = v);
        self.viewport.set(v);
        self.dirty.set(true);
        self.schedule_view_save();
    }
}

fn canvas() -> web_sys::Element {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id("canvas")
        .unwrap()
}
fn canvas_rect() -> web_sys::DomRect {
    canvas().get_bounding_client_rect()
}
fn client_point(event: &PointerEvent) -> Point {
    Point {
        x: event.client_x() as f64,
        y: event.client_y() as f64,
    }
}
fn world_point(event: &PointerEvent, v: Viewport) -> Point {
    let r = canvas_rect();
    screen_to_world(
        Point {
            x: event.client_x() as f64 - r.x(),
            y: event.client_y() as f64 - r.y(),
        },
        v,
    )
}
fn center() -> Point {
    let r = canvas_rect();
    Point {
        x: r.width() / 2.0,
        y: r.height() / 2.0,
    }
}
fn port_name(port: Port) -> &'static str {
    match port {
        Port::Top => "top",
        Port::Right => "right",
        Port::Bottom => "bottom",
        Port::Left => "left",
    }
}
fn input_focused(event: &KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|el| {
            el.closest("input,textarea,select,[contenteditable=true]")
                .ok()
                .flatten()
                .is_some()
        })
}
fn install_events(ui: Ui) {
    let key = Closure::<dyn Fn(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        if event.is_composing() || event.key_code() == 229 {
            return;
        }
        if event.key() == "Escape" {
            ui.cancel();
            ui.ai_open.set(false);
            return;
        }
        if input_focused(&event) || ui.ai_open.get_untracked() {
            return;
        }
        if (event.ctrl_key() || event.meta_key()) && event.key().eq_ignore_ascii_case("z") {
            event.prevent_default();
            ui.history(event.shift_key());
        } else if event.key() == "Delete" || event.key() == "Backspace" {
            event.prevent_default();
            ui.delete();
        }
    });
    let window = web_sys::window().unwrap();
    window
        .add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref())
        .unwrap();
    key.forget();
    let blur = Closure::<dyn Fn(web_sys::Event)>::new(move |_| ui.cancel());
    window
        .add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref())
        .unwrap();
    blur.forget();
    let unload = Closure::<dyn Fn(web_sys::BeforeUnloadEvent)>::new(
        move |event: web_sys::BeforeUnloadEvent| {
            if ui.dirty.get_untracked() {
                event.prevent_default();
                event.set_return_value("");
            }
        },
    );
    window
        .add_event_listener_with_callback("beforeunload", unload.as_ref().unchecked_ref())
        .unwrap();
    unload.forget();
}

#[component]
pub fn App() -> impl IntoView {
    let ui = Ui::new();
    install_events(ui);
    view! {
        <div class="app">
            <header class="topbar">
                <div class="brand"><span class="brand-mark">"D"</span><div class="brand-text"><strong>"DFD Canvas"</strong><span>"データの流れを、明確に。"</span></div></div>
                <label class="document-title"><span class="eyebrow">"DOCUMENT"</span><input id="document-name" aria-label="文書名" prop:value=move||ui.title.get() on:change=move|e|ui.property(None,false,event_target_value(&e))/></label>
                <nav class="toolbar" aria-label="ファイル操作">
                    <button id="new" on:click=move |_|{if ui.may_replace(){ui.replace(Document::default());}}>"新規"</button>
                    <button id="open" on:click=move |_|{web_sys::window().unwrap().document().unwrap().get_element_by_id("file-input").unwrap().unchecked_into::<web_sys::HtmlInputElement>().click();}>"読み込む"</button>
                    <button id="save" title="持ち運び・バックアップ用" on:click=move |_|ui.save(false)>"JSONを保存"</button>
                    <button id="ai-export" class="primary" on:click=move |_|{ui.refresh_ai();ui.ai_open.set(true);}>"AI向けJSON"</button>
                </nav>
                <input id="file-input" type="file" accept=".json,.dfd.json,application/json" hidden on:change=move|event|{
                    let input=event_target::<web_sys::HtmlInputElement>(&event);let file=input.files().and_then(|files|files.get(0));input.set_value("");
                    if let Some(file)=file {
                        if file.size()>MAX_FILE_BYTES as f64{ui.error.set("ファイルが大きすぎます。読み込み上限は5 MiBです".into());return;}
                        spawn_local(async move {
                            let result=match JsFuture::from(file.text()).await{Ok(v)=>Document::from_json(&v.as_string().unwrap_or_default()),Err(_)=>Err("ファイルを読み込めませんでした".into())};
                            match result{Ok(doc)=>{if ui.may_replace(){ui.replace(doc);}},Err(e)=>ui.error.set(e)}
                        });
                    }
                }/>
            </header>
            <Show when=move||!ui.error.get().is_empty()><div class="error-banner" role="alert"><span>{move||ui.error.get()}</span><button aria-label="エラーを閉じる" on:click=move |_|ui.error.set(String::new())>"×"</button></div></Show>
            <main class="main-layout">
                <section class="workspace" aria-label="DFDエディタ">
                    <div class="canvas-toolbar">
                        <div class="toolbar" aria-label="ノードを追加">
                            <span class="eyebrow">"追加"</span>
                            <button id="add-process" on:click=move |_|ui.add(NodeKind::Process)><span class="node-icon process"></span>"処理"</button>
                            <button id="add-store" on:click=move |_|ui.add(NodeKind::DataStore)><span class="node-icon data_store"></span>"データストア"</button>
                            <button id="add-entity" on:click=move |_|ui.add(NodeKind::ExternalEntity)><span class="node-icon external_entity"></span>"外部主体"</button>
                            <button id="create-group" disabled=move||!ui.nodes.with(|nodes|nodes.iter().any(|vm|vm.meta.with(|n|ui.selection.with(|s|s.contains(&n.id))))) on:click=move |_|ui.create_group()>"グループ作成"</button>
                            <span class="separator"></span>
                            <button id="undo" title="Ctrl / Cmd + Z" disabled=move||!ui.undo.get() on:click=move |_|ui.history(false)>"↶ 戻す"</button>
                            <button id="redo" title="Ctrl / Cmd + Shift + Z" disabled=move||!ui.redo.get() on:click=move |_|ui.history(true)>"↷"</button>
                            <button id="delete" disabled=move||ui.selection.get().is_empty() on:click=move |_|ui.delete()>"削除"</button>
                        </div>
                        <div class="toolbar zoom-controls"><button id="zoom-out" aria-label="縮小" on:click=move |_|ui.zoom(1.0/1.2,center())>"−"</button><span class="zoom-label">{move||format!("{}%",(ui.viewport.get().zoom*100.0).round())}</span><button id="zoom-in" aria-label="拡大" on:click=move |_|ui.zoom(1.2,center())>"＋"</button><button id="fit" on:click=move |_|ui.fit()>"全体表示"</button></div>
                    </div>
                    <div class="canvas-wrap">
                        <svg id="canvas" tabindex="0" aria-label="データフロー図。空白ドラッグでパン、ホイールでズーム" on:pointerdown=move|e|ui.start_pan(e) on:pointermove=move|e|ui.pointer_move(e) on:pointerup=move|e|ui.finish(e) on:pointercancel=move |_|ui.cancel() on:lostpointercapture=move |_|ui.cancel() on:wheel=move|e|{e.prevent_default();let r=canvas_rect();ui.zoom((-e.delta_y()*0.0015).exp(),Point{x:e.client_x()as f64-r.x(),y:e.client_y()as f64-r.y()});} on:contextmenu=move|e|e.prevent_default()>
                            <defs><marker id="arrow" markerWidth="9" markerHeight="9" refX="8" refY="4.5" orient="auto" markerUnits="userSpaceOnUse"><path d="M 0 0 L 9 4.5 L 0 9 Z" fill="#64748b"/></marker></defs>
                            <g id="diagram" transform=move||{let v=ui.viewport.get();format!("translate({} {}) scale({})",v.pan_x,v.pan_y,v.zoom)}>
                                <For each=move||ui.groups.get() key=|vm|vm.meta children=move|vm|view!{<GroupView ui=ui vm=vm/>}/>
                                <For each=move||ui.edges.get() key=|vm|vm.meta children=move|vm|view!{<EdgeView ui=ui vm=vm/>}/>
                                <For each=move||ui.nodes.get() key=|vm|vm.meta children=move|vm|view!{<NodeView ui=ui vm=vm/>}/>
                                <For each=move||{
                                    let selected=ui.selection.get();
                                    ui.edges.get().into_iter().filter(|vm|selected.len()==1&&vm.meta.with_untracked(|e|selected.contains(&e.id))).collect::<Vec<_>>()
                                } key=|vm|vm.meta children=move|vm|view!{<EdgeHandles ui=ui vm=vm/>}/>
                                <Show when=move||ui.draft.get().is_some()><path class="draft-edge" pointer-events="none" d=move||ui.draft.get().map(|(a,port,b)|bezier_path(a,port,b,Port::Left,0.0).0).unwrap_or_default() marker-end="url(#arrow)"/></Show>
                            </g>
                        </svg>
                        <Show when=move||ui.nodes.with(|n|n.is_empty())&&ui.groups.with(|g|g.is_empty())><div class="canvas-empty"><span class="eyebrow">"YOUR FIRST DATA FLOW"</span><strong>"データの流れを描きましょう"</strong><p>"上のボタンからノードを追加。"<br/>"接続点をドラッグして、データの流れをつなぎます。"</p></div></Show>
                        <div class="canvas-hint">"空白ドラッグで移動 · ホイールでズーム · Shift＋クリックで複数選択"</div>
                    </div>
                </section>
                <aside class="properties"><Properties ui=ui/></aside>
            </main>
            <footer class="statusbar"><span>{move||format!("{} ノード  /  {} フロー  /  {} グループ",ui.nodes.with(|n|n.len()),ui.edges.with(|e|e.len()),ui.groups.with(|g|g.len()))}</span><span role="status">{move||ui.status.get()}</span><span>"JSON保存 = 持ち運び・バックアップ"</span></footer>
            <Show when=move||ui.ai_open.get()><AiDialog ui=ui/></Show>
        </div>
    }
}

#[component]
fn GroupView(ui: Ui, vm: GroupVm) -> impl IntoView {
    let id = vm.meta.with_untracked(|g| g.id.clone());
    let sid = StoredValue::new(id.clone());
    view! {
        <g class="group" data-group-id=id class:selected=move||ui.selection.with(|s|s.contains(&sid.get_value())) class:group-candidate=move||ui.group_candidates.with(|s|s.contains(&sid.get_value())) class:invalid-placement=move||ui.invalid_placement.get()&&ui.selection.with(|s|s.contains(&sid.get_value())) transform=move||{let r=vm.rect.get();format!("translate({} {})",r.x,r.y)}>
            <title>{move||vm.meta.with(|g|format!("グループ：{}\n{}",g.name,g.description))}</title>
            <rect class="group-frame" width=move||vm.rect.get().width height=move||vm.rect.get().height/>
            <rect class="group-border" width=move||vm.rect.get().width height=move||vm.rect.get().height on:pointerdown=move|e|ui.start_group(e,vm,false)/>
            <rect class="group-header" width=move||vm.rect.get().width height=GROUP_HEADER_HEIGHT on:pointerdown=move|e|ui.start_group(e,vm,false)/>
            <text class="group-name" x="12" y="22">{move||vm.meta.with(|g|{let max=((vm.rect.get().width-24.0)/14.0).floor() as usize;if g.name.chars().count()>max{format!("{}…",g.name.chars().take(max.saturating_sub(1)).collect::<String>())}else{g.name.clone()}})}</text>
            <rect class="group-resize-handle" x=move||vm.rect.get().width-10.0 y=move||vm.rect.get().height-10.0 width="10" height="10" on:pointerdown=move|e|ui.start_group(e,vm,true)/>
        </g>
    }
}

#[component]
fn NodeView(ui: Ui, vm: NodeVm) -> impl IntoView {
    let id = vm.meta.with_untracked(|n| n.id.clone());
    let sid = StoredValue::new(id.clone());
    let kind = vm.meta.with_untracked(|n| n.kind);
    let clip = format!(
        "clip-{}",
        id.chars()
            .map(|c| format!("{:x}-", c as u32))
            .collect::<String>()
    );
    let clip_ref = format!("url(#{clip})");
    view! {
        <g class="node" data-node-id=id data-member-of=move||vm.meta.with(|n|n.group_id.clone()) class:selected=move||ui.selection.with(|s|s.contains(&sid.get_value())) class:invalid-placement=move||ui.invalid_placement.get()&&ui.selection.with(|s|s.contains(&sid.get_value())) transform=move||{let r=vm.rect.get();format!("translate({} {})",r.x,r.y)} on:pointerdown=move|e|ui.start_move(e,vm,false)>
            <title>{move||vm.meta.with(|n|format!("{}：{}\n{}",n.kind.label(),n.name,n.description))}</title>
            <rect class="node-shape" width=move||vm.rect.get().width height=move||vm.rect.get().height rx=if kind==NodeKind::Process{14}else{0}/>
            {(kind==NodeKind::DataStore).then(||view!{<path class="store-lines" d=move||{let r=vm.rect.get();format!("M 1 8 H {} M 1 {} H {}",r.width-1.0,r.height-8.0,r.width-1.0)} pointer-events="none"/>})}
            <defs><clipPath id=clip><rect x="12" y="10" width=move||(vm.rect.get().width-24.0).max(1.0) height=move||(vm.rect.get().height-20.0).max(1.0)/></clipPath></defs>
            <g clip-path=clip_ref pointer-events="none"><text class="node-kind" x="16" y=move||(vm.rect.get().height/2.0-10.0).max(19.0)>{kind.label()}</text><text class="node-name" x="16" y=move||(vm.rect.get().height/2.0+14.0).max(36.0)>{move||vm.meta.with(|n|{let max=((vm.rect.get().width-32.0)/14.0).floor()as usize;if n.name.chars().count()>max{format!("{}…",n.name.chars().take(max.saturating_sub(1)).collect::<String>())}else{n.name.clone()}})}</text></g>
            {[Port::Top,Port::Right,Port::Bottom,Port::Left].into_iter().map(move|port|view!{
                <circle class="port" data-port=port_name(port) class:candidate=move||ui.candidate.get().is_some_and(|(id,p)|id==sid.get_value()&&p==port) cx=move||{let mut r=vm.rect.get();r.x=0.0;r.y=0.0;port_position(r,port).x} cy=move||{let mut r=vm.rect.get();r.x=0.0;r.y=0.0;port_position(r,port).y} r="5" on:pointerdown=move|e|ui.start_connect(e,vm,port)/>
            }).collect_view()}
            <rect class="resize-handle" x=move||vm.rect.get().width-7.0 y=move||vm.rect.get().height-7.0 width="12" height="12" rx="2" on:pointerdown=move|e|ui.start_move(e,vm,true)/>
        </g>
    }
}
#[component]
fn EdgeView(ui: Ui, vm: EdgeVm) -> impl IntoView {
    let id = vm.meta.with_untracked(|e| e.id.clone());
    let sid = StoredValue::new(id.clone());
    let curve = Memo::new(move |_| {
        let (source, source_port, target, target_port) = edge_points(ui, vm);
        bezier_path(source, source_port, target, target_port, vm.offset.get())
    });
    view! {
        <g class="edge" data-edge-id=id class:selected=move||ui.selection.with(|s|s.contains(&sid.get_value())) on:pointerdown=move|e:PointerEvent|{e.stop_propagation();if e.button()==0{let _=canvas().unchecked_into::<web_sys::HtmlElement>().focus();ui.select(&sid.get_value(),e.shift_key());}}>
            <title>{move||vm.meta.with(|e|format!("{}\n{}",e.name,e.description))}</title>
            <path class="edge-hit" d=move||curve.with(|c|c.0.clone())/>
            <path class="edge-path" d=move||curve.with(|c|c.0.clone()) marker-end="url(#arrow)"/>
            <g transform=move||curve.with(|c|format!("translate({} {})",c.1.x,c.1.y))>
                <rect class="edge-label-bg" x="-70" y="-13" width="140" height="26" rx="5"/>
                <text class="edge-label" text-anchor="middle" y="5">{move||vm.meta.with(|e|if e.name.chars().count()>10{format!("{}…",e.name.chars().take(9).collect::<String>())}else{e.name.clone()})}</text>
            </g>
        </g>
    }
}

// Preview only changes the projection. The document and autosave keep the
// original ports until a valid drop commits one operation.
fn edge_points(ui: Ui, vm: EdgeVm) -> (Point, Port, Point, Port) {
    let ports = vm.ports.get();
    let mut source_port = ports.source_port;
    let mut target_port = ports.target_port;
    let mut source = port_position(vm.source.get(), source_port);
    let mut target = port_position(vm.target.get(), target_port);
    if let Some(preview) = ui.edge_preview.get()
        && vm.meta.with_untracked(|e| e.id == preview.id)
    {
        match preview.end {
            EdgeEnd::Source => {
                source = preview.point;
                source_port = preview.port;
            }
            EdgeEnd::Target => {
                target = preview.point;
                target_port = preview.port;
            }
        }
    }
    (source, source_port, target, target_port)
}

#[component]
fn EdgeHandles(ui: Ui, vm: EdgeVm) -> impl IntoView {
    let points = Memo::new(move |_| edge_points(ui, vm));
    [EdgeEnd::Source, EdgeEnd::Target].into_iter().map(move |end| {
        let point = move || { let p = points.get(); if end == EdgeEnd::Source { p.0 } else { p.2 } };
        let label = if end == EdgeEnd::Source { "接続元の位置を変更：同じノードの接続点へドラッグ" } else { "接続先の位置を変更：同じノードの接続点へドラッグ" };
        view! {
            <circle class="edge-endpoint" data-edge-handle=vm.meta.with_untracked(|e|e.id.clone()) data-end=if end==EdgeEnd::Source{"source"}else{"target"}
                aria-label=label cx=move||point().x cy=move||point().y r=move||7.0/ui.viewport.get().zoom
                on:pointerdown=move|e|ui.start_reconnect(e,vm,end)>
                <title>{label}</title>
            </circle>
        }
    }).collect_view()
}

#[component]
fn Properties(ui: Ui) -> impl IntoView {
    let selected = Memo::new(move |_| {
        ui.revision.get();
        let s = ui.selection.get();
        if s.len() == 1 {
            s.first().cloned()
        } else {
            None
        }
    });
    view! {
        <div class="panel-section"><span class="eyebrow">"INSPECTOR"</span><h2>"プロパティ"</h2><p class="selection-summary">{move||if ui.selection.get().is_empty(){"図の要素を選択して編集".into()}else{format!("{} 件を選択中",ui.selection.get().len())}}</p></div>
        {move||{ui.revision.get();selected.get().map(|id|{
            let info=ui.runtime.with_value(|rt|{let e=rt.editor.borrow();if let Some(n)=e.document.graph.nodes.iter().find(|n|n.id==id){let group=n.group_id.as_ref().and_then(|id|e.document.graph.groups.iter().find(|g| &g.id==id)).map(|g|g.name.as_str()).unwrap_or("未所属");Some((n.kind.label().to_string(),n.name.clone(),n.description.clone(),format!("所属：{group}")))}else if let Some(g)=e.document.graph.groups.iter().find(|g|g.id==id){Some(("グループ".into(),g.name.clone(),g.description.clone(),format!("所属ノード：{} 件。グループを削除してもノードと接続は残ります。",e.document.graph.nodes.iter().filter(|n|n.group_id.as_ref()==Some(&g.id)).count())))}else{e.document.graph.edges.iter().find(|e|e.id==id).map(|edge|{let name=|id:&str|e.document.graph.nodes.iter().find(|n|n.id==id).map(|n|n.name.as_str()).unwrap_or(id).to_string();("データフロー".into(),edge.name.clone(),edge.description.clone(),format!("{} → {}",name(&edge.source),name(&edge.target)))})}});
            info.map(|(kind,name,description,connection)|{let sid=StoredValue::new(id.clone());view!{
                <div class="panel-section"><span class="kind-badge">{kind}</span><p class="item-id">{id}</p>
                <label class="field"><span>"名前"</span><input id="prop-name" prop:value=name on:change=move|e|ui.property(Some(sid.get_value()),false,event_target_value(&e))/></label>
                <label class="field"><span>"説明"</span><textarea id="prop-description" rows="6" placeholder="この要素の役割や、データの内容を説明します" prop:value=description on:change=move|e|ui.property(Some(sid.get_value()),true,event_target_value(&e))></textarea></label>
                <p class="connection-info">{connection}</p><p class="field-help">"入力欄を離れると編集が確定します。"</p></div>
            }})
        })}}
        <div class="panel-section help-section"><h3>"つなぐ、動かす、伝える"</h3><p>"ノードの丸い接続点から、別の接続点へドラッグして接続します。"</p><p>"線を選ぶと両端に丸が表示されます。丸を同じノードの別の接続点へドラッグすると、接続位置を付け替えられます。"</p><p>"右下のハンドルでサイズを変更。Shift＋クリックでまとめて選択できます。"</p><p>"選択したノードからグループを作成。ノード全体を枠の内外へドラッグすると所属が変わります。見出し・境界でまとめて移動、右下で枠のサイズを変更できます。"</p><p>"名前・説明・所属は、そのままAI向けJSONへ。"</p></div>
    }
}

#[component]
fn AiDialog(ui: Ui) -> impl IntoView {
    let copy_status = RwSignal::new(String::new());
    view! {
        <div class="modal-backdrop"><section class="json-dialog" role="dialog" aria-modal="true" aria-labelledby="ai-title"><div class="dialog-heading"><div><span class="eyebrow">"STRUCTURED DATA"</span><h2 id="ai-title">"AI向けJSON"</h2></div><button id="ai-close" aria-label="閉じる" on:click=move |_|ui.ai_open.set(false)>"閉じる"</button></div>
            <p>"現在の文書から生成した意味情報です。座標・サイズ・接続点・ズームを含みません。"</p>
            <textarea id="ai-json" aria-label="AI向けJSONの内容" readonly prop:value=move||ui.ai_text.get() spellcheck="false"></textarea>
            <p role="status">{move||copy_status.get()}</p><div class="dialog-actions"><button id="ai-select" on:click=move |_|{web_sys::window().unwrap().document().unwrap().get_element_by_id("ai-json").unwrap().unchecked_into::<web_sys::HtmlTextAreaElement>().select();}>"テキストを全選択"</button><button id="ai-save" on:click=move |_|ui.save(true)>"ファイル出力"</button><button id="ai-copy" class="primary" on:click=move |_|spawn_local(async move{
                // Clipboard can be absent outside a secure context; Reflect avoids invoking an absent API.
                let navigator=web_sys::window().unwrap().navigator();
                let result=if js_sys::Reflect::get(navigator.as_ref(),&"clipboard".into()).ok().is_none_or(|v|v.is_undefined()||v.is_null()){Err(wasm_bindgen::JsValue::NULL)}else{JsFuture::from(navigator.clipboard().write_text(&ui.ai_text.get_untracked())).await};
                copy_status.set(if result.is_ok(){"コピーしました"}else{"コピーできませんでした。テキストを全選択してコピーするか、ファイル出力を使ってください。"}.into());
            })>"コピー"</button></div>
        </section></div>
    }
}
