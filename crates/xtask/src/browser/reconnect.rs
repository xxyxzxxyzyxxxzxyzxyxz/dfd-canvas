use super::*;
use dfd_canvas::{
    editor::Editor,
    model::{Document, NodeKind, Port},
};

fn edge(id: &str) -> String {
    format!("g[data-edge-id={}]", json!(id))
}

fn handle(id: &str, end: &str) -> String {
    format!(
        ".edge-endpoint[data-edge-handle={}][data-end={}]",
        json!(id),
        json!(end)
    )
}

async fn select_edge(page: &Page, id: &str, label: bool) -> Result<()> {
    if label {
        click(page, &format!("{} .edge-label-bg", edge(id))).await?;
    } else {
        let selector = format!("{} .edge-hit", edge(id));
        let point: [f64; 2] = eval(page, format!("() => {{const e=document.querySelector({});const p=e.getPointAtLength(e.getTotalLength()*.32).matrixTransform(e.getScreenCTM());return [p.x,p.y];}}", json!(selector))).await?;
        click_at(page, (point[0], point[1]), 0).await?;
    }
    ensure!(
        count(page, &format!("{}.selected", edge(id))).await? == 1,
        "Selecting the edge by curve/label selected a different edge"
    );
    ensure!(
        count(page, ".edge-endpoint").await? == 2,
        "A selected edge must expose exactly its source and target handles"
    );
    Ok(())
}

async fn begin_drag(page: &Page, id: &str, end: &str, target: Point) -> Result<()> {
    let source = center(page, &handle(id, end)).await?;
    mouse(page, "mouseMoved", source, "none", 0, 0).await?;
    mouse(page, "mousePressed", source, "left", 1, 0).await?;
    mouse(page, "mouseMoved", target, "left", 1, 0).await
}

async fn stored_document(page: &Page) -> Result<Value> {
    let saved: String = eval(
        page,
        format!("() => localStorage.getItem({})", json!(RECOVERY_KEY)),
    )
    .await?;
    Ok(serde_json::from_str(&saved)?)
}

pub(super) async fn acceptance(browser: &Browser, url: &str, output: &Path) -> Result<Value> {
    let session = Session::new(browser, output, None).await?;
    session.navigate(url).await?;
    let page = &session.page;
    let mut editor = Editor::new(Document::default());
    let source = editor.add_node(NodeKind::Process, 160.0, 180.0).unwrap();
    let target = editor.add_node(NodeKind::DataStore, 600.0, 370.0).unwrap();
    let outsider = editor
        .add_node(NodeKind::ExternalEntity, 720.0, 100.0)
        .unwrap();
    let selected = editor
        .add_edge(&source, &target, Port::Right, Port::Left)
        .unwrap();
    editor
        .add_edge(&source, &target, Port::Bottom, Port::Top)
        .unwrap();
    editor.document.graph.edges[0].name = "注文情報".into();
    editor.document.graph.edges[0].description = "接続位置だけを変更するフロー".into();
    editor.document.graph.edges[1].name = "別のフロー".into();
    editor.document.graph.edges[1].description = "同じノード間の平行エッジを保持する".into();
    let baseline = serde_json::to_value(&editor.document)?;
    let fixture_path = output.join("reconnect-initial.dfd.json");
    write_json(&fixture_path, &baseline)?;
    load_document(page, &fixture_path).await?;
    ensure!(
        count(page, ".edge-endpoint").await? == 0,
        "Unselected edges exposed endpoint handles"
    );
    click(page, "#ai-export").await?;
    let original_ai = value(page, "#ai-json").await?;
    click(page, "#ai-close").await?;
    sleep(Duration::from_millis(650)).await;
    ensure!(
        stored_document(page).await? == baseline,
        "Autosave did not reach the baseline before cancellation checks"
    );

    // Each rejected gesture starts with no history: a no-op must not create Undo.
    for (end, node_id, original_port, new_port) in [
        ("source", &source, "right", "top"),
        ("target", &target, "left", "bottom"),
    ] {
        for cancellation in [
            "same-port",
            "wrong-node",
            "blank",
            "Escape",
            "pointercancel",
            "lostpointercapture",
            "blur",
        ] {
            select_edge(page, &selected, end == "target").await?;
            let old_path = attribute(page, &format!("{} .edge-path", edge(&selected)), "d").await?;
            let destination = match cancellation {
                "same-port" => port(page, node_id, original_port).await?,
                "wrong-node" => port(page, &outsider, "left").await?,
                "blank" => blank(page).await?,
                _ => port(page, node_id, new_port).await?,
            };
            begin_drag(page, &selected, end, destination).await?;
            match cancellation {
                "Escape" => press(page, "Escape", 0).await?,
                "blur" => js(page, "() => window.dispatchEvent(new Event('blur'))").await?,
                "pointercancel" | "lostpointercapture" => {
                    // Cancellation is injected; the preceding gesture uses real input.
                    js(page, format!("() => document.querySelector('#canvas').dispatchEvent(new PointerEvent({},{{pointerId:1,bubbles:true}}))", json!(cancellation))).await?;
                }
                _ => {}
            }
            mouse(page, "mouseReleased", destination, "left", 0, 0).await?;
            ensure!(
                attribute(page, &format!("{} .edge-path", edge(&selected)), "d").await? == old_path,
                "{end}/{cancellation} did not restore the original curve"
            );
            ensure!(
                stored_document(page).await? == baseline,
                "{end}/{cancellation} modified autosave"
            );
            check(
                page,
                "document.querySelector('#undo').disabled && document.querySelector('#redo').disabled",
                "A rejected/cancelled endpoint drag polluted Undo/Redo history",
            )
            .await?;
        }
        ensure!(
            save_document(browser, page, output, &format!("reconnect-{end}-cancelled")).await?
                == baseline,
            "{end} rejected/cancelled endpoint drags modified the document"
        );
    }

    // Screen coordinates must still resolve to the original node's world ports.
    let pan = blank(page).await?;
    drag(page, pan, (pan.0 + 40.0, pan.1 - 20.0), 10).await?;
    click(page, "#zoom-in").await?;
    let transformed = save_document(browser, page, output, "reconnect-transformed").await?;
    ensure!(
        transformed["layout"]["viewport"] != baseline["layout"]["viewport"],
        "Relocation fixture did not pan/zoom"
    );
    sleep(Duration::from_millis(650)).await;
    ensure!(
        stored_document(page).await? == transformed,
        "Autosave did not reach the baseline before the pending gesture"
    );
    select_edge(page, &selected, false).await?;
    let old_path = attribute(page, &format!("{} .edge-path", edge(&selected)), "d").await?;
    let new_source = port(page, &source, "top").await?;
    begin_drag(page, &selected, "source", new_source).await?;
    ensure!(
        attribute(page, &format!("{} .edge-path", edge(&selected)), "d").await? != old_path,
        "Endpoint drag did not preview the selected edge's curve"
    );
    sleep(Duration::from_millis(650)).await;
    ensure!(
        stored_document(page).await? == transformed,
        "Pending endpoint preview leaked into autosave"
    );
    mouse(page, "mouseReleased", new_source, "left", 0, 0).await?;
    let mut expected_source = transformed.clone();
    expected_source["layout"]["edges"][&selected]["source_port"] = json!("top");
    let source_changed = save_document(browser, page, output, "reconnect-source").await?;
    ensure!(
        source_changed == expected_source,
        "Source relocation must change only the selected edge's source_port"
    );
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "reconnect-source-undone").await? == transformed,
        "Source relocation did not undo in one action"
    );
    click(page, "#redo").await?;
    ensure!(
        save_document(browser, page, output, "reconnect-source-redone").await? == source_changed,
        "Source relocation redo did not restore the exact document"
    );

    select_edge(page, &selected, true).await?;
    drag(
        page,
        center(page, &handle(&selected, "target")).await?,
        port(page, &target, "bottom").await?,
        15,
    )
    .await?;
    let mut expected_target = source_changed.clone();
    expected_target["layout"]["edges"][&selected]["target_port"] = json!("bottom");
    let relocated = save_document(browser, page, output, "reconnect-both").await?;
    ensure!(
        relocated == expected_target,
        "Target relocation must change only the selected edge's target_port"
    );
    graph_equal(&relocated, &baseline)?;
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "reconnect-target-undone").await? == source_changed,
        "Target relocation did not undo in one action"
    );
    click(page, "#redo").await?;
    ensure!(
        save_document(browser, page, output, "reconnect-target-redone").await? == relocated,
        "Target relocation redo did not restore the exact document"
    );
    click(page, "#ai-export").await?;
    ensure!(
        value(page, "#ai-json").await? == original_ai,
        "Endpoint relocation changed semantic AI JSON"
    );
    click(page, "#ai-close").await?;
    select_edge(page, &selected, true).await?;
    screenshot(page, output, "reconnect").await?;

    click(page, "#new").await?;
    load_document(page, &output.join("reconnect-both.dfd.json")).await?;
    ensure!(
        save_document(browser, page, output, "reconnect-reloaded").await? == relocated,
        "File roundtrip did not preserve relocated endpoints"
    );
    sleep(Duration::from_millis(650)).await;
    page.reload().await?;
    wait(
        page,
        "document.querySelectorAll('g[data-edge-id]').length === 2",
    )
    .await?;
    ensure!(
        save_document(browser, page, output, "reconnect-recovered").await? == relocated,
        "Autosave recovery did not preserve relocated endpoints"
    );

    // At 10%, multiple ports fit in the hit radius; the nearest must win.
    let mut small = baseline.clone();
    small["layout"]["viewport"] = json!({"pan_x":200.0,"pan_y":200.0,"zoom":0.1});
    let small_path = output.join("reconnect-min-zoom.dfd.json");
    write_json(&small_path, &small)?;
    load_document(page, &small_path).await?;
    select_edge(page, &selected, false).await?;
    for (end, node_id, new_port) in [("source", &source, "bottom"), ("target", &target, "right")] {
        drag(
            page,
            center(page, &handle(&selected, end)).await?,
            port(page, node_id, new_port).await?,
            10,
        )
        .await?;
        small["layout"]["edges"][&selected][format!("{end}_port")] = json!(new_port);
        ensure!(
            save_document(browser, page, output, &format!("reconnect-min-zoom-{end}")).await?
                == small,
            "At minimum zoom, {end} relocation did not choose the nearest port"
        );
    }
    session.close(browser).await?;
    Ok(json!({
        "checks": [
            "Show source and target handles for edges selected by their lines or labels",
            "Preserve the document and history when dropping either endpoint on the same port, another node, or empty space",
            "Cancel source and target endpoint drags on Esc, pointercancel, loss of pointer capture, or blur",
            "Reassign ports on the same node after panning and zooming; preview curves; protect autosave before committing",
            "Preserve parallel edges, IDs, names, descriptions, connected nodes, and AI JSON; undo/redo each endpoint change in one step",
            "Save and reload JSON with both endpoints reassigned; recover from autosave",
            "Select the correct source or target among closely spaced connection ports at minimum zoom"
        ],
        "page_errors": []
    }))
}
