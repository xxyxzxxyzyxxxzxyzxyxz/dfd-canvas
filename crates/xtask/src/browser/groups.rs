use super::*;
use dfd_canvas::{
    editor::Editor,
    model::{Document, NodeKind, Port},
};

fn group(id: &str) -> String {
    format!("g.group[data-group-id={}]", json!(id))
}

async fn world(page: &Page, x: f64, y: f64) -> Result<Point> {
    let p: [f64; 2] = eval(page, format!("() => {{const p=new DOMPoint({x},{y}).matrixTransform(document.querySelector('#diagram').getScreenCTM());return [p.x,p.y];}}" )).await?;
    Ok((p[0], p[1]))
}

async fn move_to(page: &Page, id: &str, x: f64, y: f64) -> Result<()> {
    drag(
        page,
        body_center(page, id).await?,
        world(page, x + 80.0, y + 40.0).await?,
        15,
    )
    .await
}

async fn dismiss_error(page: &Page) -> Result<()> {
    ensure!(
        count(page, ".error-banner").await? == 1,
        "Rejected placement must explain the reason"
    );
    click(page, ".error-banner button").await
}

fn member<'a>(doc: &'a Value, id: &str) -> &'a Value {
    &doc["graph"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap()["group_id"]
}

async fn geometry(page: &Page) -> Result<Value> {
    eval(page, "() => [...document.querySelectorAll('g.node,g.group')].map(e=>{const r=e.querySelector('.node-shape,.group-frame');return [e.getAttribute('transform'),e.getAttribute('data-member-of'),r.getAttribute('width'),r.getAttribute('height')];})").await
}

pub(super) async fn acceptance(browser: &Browser, url: &str, output: &Path) -> Result<Value> {
    let session = Session::new(browser, output, None).await?;
    session.navigate(url).await?;
    let page = &session.page;
    let mut e = Editor::new(Document::default());
    let a = e.add_node(NodeKind::Process, 100.0, 150.0).unwrap();
    let b = e.add_node(NodeKind::DataStore, 360.0, 180.0).unwrap();
    let c = e.add_node(NodeKind::ExternalEntity, 800.0, 200.0).unwrap();
    e.add_edge(&a, &c, Port::Right, Port::Left).unwrap();
    let fixture_path = output.join("groups-initial.dfd.json");
    write_json(&fixture_path, &serde_json::to_value(&e.document)?)?;
    load_document(page, &fixture_path).await?;
    click_at(page, body_center(page, &a).await?, 0).await?;
    click_at(page, body_center(page, &b).await?, 8).await?;
    click(page, "#create-group").await?;
    let first = save_document(browser, page, output, "groups-created").await?;
    ensure!(first["schema_version"] == 2, "New documents must use v2");
    let g = first["graph"]["groups"][0]["id"]
        .as_str()
        .context("created group")?
        .to_owned();
    ensure!(
        member(&first, &a) == &g && member(&first, &b) == &g,
        "Group creation must assign selected nodes"
    );
    click(page, "#undo").await?;
    ensure!(
        count(page, "g.group").await? == 0,
        "Group creation is one undo"
    );
    click(page, "#redo").await?;
    click(page, &format!("{} .group-header", group(&g))).await?;
    commit_field(page, "#prop-name", "クラウドのリソース").await?;
    commit_field(
        page,
        "#prop-description",
        "所属と配置を分けて保存するグループです。",
    )
    .await?;
    let named = save_document(browser, page, output, "groups-named").await?;
    ensure!(
        named["graph"]["groups"][0]["name"] == "クラウドのリソース",
        "Group name edit failed"
    );
    let handle = center(page, &format!("{} .group-resize-handle", group(&g))).await?;
    drag(page, handle, (handle.0 + 80.0, handle.1 + 240.0), 20).await?;
    let enlarged = save_document(browser, page, output, "groups-enlarged").await?;
    ensure!(
        enlarged["layout"]["groups"][&g]["height"].as_f64()
            > named["layout"]["groups"][&g]["height"].as_f64(),
        "Group resize failed"
    );
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "groups-resize-undone").await? == named,
        "Group resize undo failed"
    );
    click(page, "#redo").await?;

    click_at(page, body_center(page, &c).await?, 0).await?;
    click(page, "#create-group").await?;
    commit_field(page, "#prop-name", "移籍先").await?;
    let base = save_document(browser, page, output, "groups-baseline").await?;
    let h = base["graph"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] != g)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut checks = vec![
        "Create groups from selected nodes; use Japanese names and descriptions; resize frames; undo/redo each operation",
    ];

    sleep(Duration::from_millis(650)).await;
    let start = body_center(page, &c).await?;
    let end = world(page, 420.0, 410.0).await?;
    mouse(page, "mousePressed", start, "left", 1, 0).await?;
    mouse(page, "mouseMoved", end, "left", 1, 0).await?;
    ensure!(
        count(page, &format!("{}.group-candidate", group(&g))).await? == 1,
        "Drop candidate is not highlighted"
    );
    ensure!(
        attribute(page, &node(&c), "data-member-of").await? == h,
        "Membership changed before drop"
    );
    sleep(Duration::from_millis(650)).await;
    let recovery_text: String = eval(
        page,
        format!("() => localStorage.getItem({})", json!(RECOVERY_KEY)),
    )
    .await?;
    let recovery: Value = serde_json::from_str(&recovery_text)?;
    ensure!(recovery == base, "Autosave wrote a pending group gesture");
    mouse(page, "mouseReleased", end, "left", 0, 0).await?;
    let transferred = save_document(browser, page, output, "groups-transferred").await?;
    ensure!(
        member(&transferred, &c) == &g,
        "Direct group transfer failed"
    );
    ensure!(
        count(page, "g.group").await? == 2,
        "Empty source group was removed"
    );
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "groups-transfer-undone").await? == base,
        "Transfer and position must undo together"
    );
    click(page, "#redo").await?;
    move_to(page, &c, 800.0, 580.0).await?;
    let outside = save_document(browser, page, output, "groups-left").await?;
    ensure!(
        member(&outside, &c).is_null(),
        "Leaving frame must clear membership"
    );
    move_to(page, &c, 340.0, 370.0).await?;
    ensure!(
        member(
            &save_document(browser, page, output, "groups-joined").await?,
            &c
        ) == &g,
        "Ungrouped node failed to join"
    );
    move_to(page, &c, 800.0, 200.0).await?;
    ensure!(
        save_document(browser, page, output, "groups-returned").await? == base,
        "Return transfer changed other elements"
    );
    checks.push("Show drop candidates; preserve membership and autosave before committing; join, leave, or transfer directly between groups; retain empty groups");

    // Border/header crossings roll back position and membership together.
    for (x, y) in [(600.0, 350.0), (100.0, 110.0)] {
        move_to(page, &c, x, y).await?;
        ensure!(
            save_document(browser, page, output, "groups-rejected-node").await? == base,
            "Invalid node drop changed document"
        );
        dismiss_error(page).await?;
    }
    let header = center(page, &format!("{} .group-header", group(&g))).await?;
    drag(page, header, (header.0 + 450.0, header.1), 20).await?;
    ensure!(
        save_document(browser, page, output, "groups-overlap-move").await? == base,
        "Overlapping group move accepted"
    );
    dismiss_error(page).await?;
    let handle = center(page, &format!("{} .group-resize-handle", group(&g))).await?;
    drag(page, handle, (handle.0 + 380.0, handle.1), 20).await?;
    ensure!(
        save_document(browser, page, output, "groups-overlap-resize").await? == base,
        "Overlapping group resize accepted"
    );
    dismiss_error(page).await?;
    let handle = center(page, &format!("{} .group-resize-handle", group(&g))).await?;
    drag(page, handle, (handle.0 - 400.0, handle.1 - 400.0), 20).await?;
    let shrunk = save_document(browser, page, output, "groups-shrunk").await?;
    Document::from_json(&serde_json::to_string(&shrunk)?).map_err(|e| anyhow!(e))?;
    ensure!(
        shrunk["layout"]["nodes"] == base["layout"]["nodes"],
        "Shrink moved member nodes"
    );
    ensure!(
        shrunk["layout"]["groups"][&g] == named["layout"]["groups"][&g],
        "Shrink did not stop at members"
    );
    click(page, "#undo").await?;
    // Remove the other frame, then prove movement/enlargement cannot capture its node.
    click(page, &format!("{} .group-header", group(&h))).await?;
    click(page, "#delete").await?;
    let no_second = save_document(browser, page, output, "groups-with-outsider").await?;
    ensure!(
        member(&no_second, &c).is_null(),
        "Deleting frame must clear member relationship"
    );
    for resize in [false, true] {
        let selector = if resize {
            ".group-resize-handle"
        } else {
            ".group-header"
        };
        let start = center(page, &format!("{} {selector}", group(&g))).await?;
        drag(page, start, (start.0 + 400.0, start.1), 20).await?;
        ensure!(
            save_document(browser, page, output, "groups-capture-rejected").await? == no_second,
            "Frame captured unrelated node"
        );
        dismiss_error(page).await?;
    }
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "groups-restored-second").await? == base,
        "Rejected operations polluted history"
    );
    checks.push(
        "Handle drops on borders and headers and overlapping frames; reject capturing nonmember nodes; limit shrinking to fit member nodes",
    );

    let old_path = attribute(page, "g[data-edge-id] .edge-path", "d").await?;
    let start = center(page, &format!("{} .group-header", group(&g))).await?;
    drag(page, start, (start.0 + 30.0, start.1 + 25.0), 20).await?;
    let moved = save_document(browser, page, output, "groups-moved").await?;
    graph_equal(&moved, &base)?;
    for id in [&a, &b] {
        ensure!(
            (moved["layout"]["nodes"][id]["x"].as_f64().unwrap()
                - base["layout"]["nodes"][id]["x"].as_f64().unwrap()
                - 30.0)
                .abs()
                < 0.01,
            "Group failed to move member"
        );
        ensure!(
            (moved["layout"]["nodes"][id]["y"].as_f64().unwrap()
                - base["layout"]["nodes"][id]["y"].as_f64().unwrap()
                - 25.0)
                .abs()
                < 0.01,
            "Relative node position changed"
        );
    }
    ensure!(
        old_path != attribute(page, "g[data-edge-id] .edge-path", "d").await?,
        "Edges did not follow group move"
    );
    click(page, "#ai-export").await?;
    let ai: Value = serde_json::from_str(&value(page, "#ai-json").await?)?;
    graph_equal(&ai, &base)?;
    ensure!(
        ai.get("layout").is_none(),
        "AI JSON contains frame coordinates"
    );
    click(page, "#ai-close").await?;
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "groups-move-undone").await? == base,
        "Group move must undo in one step"
    );
    checks.push("Preserve relative positions and update connected edges when moving groups; preserve semantic JSON; restore with one undo");

    click(page, &format!("{} .group-header", group(&g))).await?;
    click_at(page, body_center(page, &a).await?, 8).await?;
    let start = center(page, &format!("{} .group-header", group(&g))).await?;
    drag(page, start, (start.0 + 12.0, start.1 + 8.0), 12).await?;
    let mixed = save_document(browser, page, output, "groups-mixed-selection").await?;
    ensure!(
        mixed["layout"]["nodes"][&a]["x"].as_f64().unwrap() == 112.0,
        "Selecting frame and member moved member twice"
    );
    ensure!(
        mixed["layout"]["nodes"][&b]["x"].as_f64().unwrap() == 372.0,
        "Mixed selection omitted other members"
    );
    click(page, "#undo").await?;
    click_at(page, blank(page).await?, 0).await?;
    click_at(page, body_center(page, &a).await?, 0).await?;
    click_at(page, body_center(page, &b).await?, 8).await?;
    let start = body_center(page, &a).await?;
    drag(page, start, (start.0 + 10.0, start.1 + 10.0), 12).await?;
    let multiple = save_document(browser, page, output, "groups-multiple-nodes").await?;
    ensure!(
        multiple["layout"]["groups"] == base["layout"]["groups"],
        "Moving selected members moved frame"
    );
    ensure!(
        multiple["layout"]["nodes"][&b]["x"].as_f64().unwrap() == 370.0,
        "Multi-node drag missed a member"
    );
    click(page, "#undo").await?;
    let handle = center(page, &format!("{} .resize-handle", node(&c))).await?;
    drag(page, handle, (handle.0 + 30.0, handle.1 + 20.0), 12).await?;
    ensure!(
        save_document(browser, page, output, "groups-node-resize-rejected").await? == base,
        "Resizing a member outside its group was accepted"
    );
    dismiss_error(page).await?;
    checks.push("Select groups and member nodes together; move multiple member nodes; validate boundaries when enlarging nodes");

    // Exercise cancellation for all layout gestures, including after the save delay.
    let original_geometry = geometry(page).await?;
    for gesture in ["node", "group", "resize"] {
        for cancellation in ["Escape", "pointercancel", "blur", "lostpointercapture"] {
            let start = match gesture {
                "node" => body_center(page, &c).await?,
                "group" => center(page, &format!("{} .group-header", group(&g))).await?,
                _ => center(page, &format!("{} .group-resize-handle", group(&g))).await?,
            };
            let end = if gesture == "node" {
                world(page, 420.0, 410.0).await?
            } else {
                (start.0 + 30.0, start.1 + 20.0)
            };
            mouse(page, "mousePressed", start, "left", 1, 0).await?;
            mouse(page, "mouseMoved", end, "left", 1, 0).await?;
            match cancellation {
                "Escape" => press(page,"Escape",0).await?,
                "blur" => js(page,"() => window.dispatchEvent(new Event('blur'))").await?,
                event => js(page,format!("() => document.querySelector('#canvas').dispatchEvent(new PointerEvent({},{{pointerId:1,bubbles:true}}))",json!(event))).await?,
            }
            mouse(page, "mouseReleased", end, "left", 0, 0).await?;
            ensure!(
                geometry(page).await? == original_geometry,
                "{gesture}/{cancellation} failed to restore geometry and membership"
            );
        }
    }
    ensure!(
        save_document(browser, page, output, "groups-cancelled").await? == base,
        "Cancelled gestures changed the saved document"
    );
    checks.push("Cancel node moves, frame moves, and frame resizes on Esc, pointercancel, blur, or loss of pointer capture");

    // Content remains interactive: ports connect, blank content pans, borders select.
    drag(
        page,
        port(page, &a, "right").await?,
        port(page, &b, "left").await?,
        20,
    )
    .await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 2,
        "Group interior blocked connection"
    );
    click(page, "#undo").await?;
    let start = world(page, 120.0, 470.0).await?;
    drag(page, start, (start.0 + 25.0, start.1 - 15.0), 20).await?;
    let panned = save_document(browser, page, output, "groups-pan").await?;
    ensure!(
        panned["layout"]["viewport"] != base["layout"]["viewport"],
        "Empty frame interior blocked pan"
    );
    ensure!(
        panned["layout"]["groups"] == base["layout"]["groups"],
        "Interior pan moved frame"
    );
    click(page, "#undo").await?;
    click_at(page, world(page, 84.0, 400.0).await?, 0).await?;
    ensure!(
        count(page, &format!("{}.selected", group(&g))).await? == 1,
        "Frame border is not selectable"
    );
    click(page, "#zoom-in").await?;
    click(page, "#fit").await?;
    check(page,"(() => {const canvas=document.querySelector('#canvas').getBoundingClientRect(); return [...document.querySelectorAll('.group-frame,.group-header')].every(e=>{const r=e.getBoundingClientRect();return r.left>=canvas.left&&r.top>=canvas.top&&r.right<=canvas.right&&r.bottom<=canvas.bottom;});})()","Fit omitted group frames or headings").await?;
    screenshot(page, output, "groups").await?;
    checks.push("Connect nodes and pan the background inside frames; select borders; zoom; fit the entire diagram including frames and headers");

    let fitted = save_document(browser, page, output, "groups-fitted").await?;
    click(page, &format!("{} .group-header", group(&g))).await?;
    click(page, "#delete").await?;
    let deleted = save_document(browser, page, output, "groups-deleted").await?;
    ensure!(
        deleted["layout"]["nodes"] == fitted["layout"]["nodes"]
            && deleted["graph"]["edges"] == fitted["graph"]["edges"],
        "Deleting group removed resources/flows"
    );
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "groups-delete-undone").await? == fitted,
        "Group delete undo changed document"
    );
    click(page, "#redo").await?;
    click(page, "#undo").await?;
    click_at(page, body_center(page, &c).await?, 0).await?;
    click(page, "#delete").await?;
    ensure!(
        count(page, "g.group").await? == 2 && count(page, "g[data-edge-id]").await? == 0,
        "Last member deletion should preserve empty frame and remove incident edge"
    );
    click(page, "#undo").await?;
    load_document(page, &output.join("groups-fitted.dfd.json")).await?;
    ensure!(
        save_document(browser, page, output, "groups-reloaded").await? == fitted,
        "Group file roundtrip changed data"
    );
    sleep(Duration::from_millis(650)).await;
    page.reload().await?;
    wait(page, "document.querySelectorAll('g.group').length === 2").await?;
    ensure!(
        save_document(browser, page, output, "groups-recovered").await? == fitted,
        "Group autosave restoration changed data"
    );
    let mut invalid = fitted.clone();
    invalid["graph"]["nodes"][0]["group_id"] = json!("missing");
    write_json(&output.join("groups-invalid.dfd.json"), &invalid)?;
    load_document(page, &output.join("groups-invalid.dfd.json")).await?;
    ensure!(
        save_document(browser, page, output, "groups-invalid-preserved").await? == fitted,
        "Invalid group import replaced active document"
    );
    dismiss_error(page).await?;
    checks.push(
        "Preserve nodes and connections when deleting groups; delete the last member node; save, reload, and recover automatically; protect the document when loading invalid input",
    );
    session.errors().await?;
    session.close(browser).await?;

    let legacy = include_str!("../../../../tests/fixtures/example.dfd.json");
    let init = format!(
        "localStorage.setItem({},{});",
        json!(RECOVERY_KEY),
        json!(legacy)
    );
    let legacy_session = Session::new(browser, output, Some(&init)).await?;
    legacy_session.navigate(url).await?;
    let migrated =
        save_document(browser, &legacy_session.page, output, "groups-v1-recovered").await?;
    ensure!(
        migrated["schema_version"] == 2 && migrated["graph"]["groups"] == json!([]),
        "Legacy recovery key did not migrate v1"
    );
    let legacy_path = output.join("groups-legacy-v1.dfd.json");
    std::fs::write(&legacy_path, legacy)?;
    load_document(&legacy_session.page, &legacy_path).await?;
    ensure!(
        save_document(browser, &legacy_session.page, output, "groups-v1-imported").await?
            == migrated,
        "Legacy file import differs from recovery"
    );
    legacy_session.close(browser).await?;

    let invalid_text = serde_json::to_string(&invalid)?;
    let init = format!(
        "localStorage.setItem({},{});",
        json!(RECOVERY_KEY),
        json!(invalid_text)
    );
    let broken = Session::new(browser, output, Some(&init)).await?;
    broken.navigate(url).await?;
    click(&broken.page, "#add-process").await?;
    sleep(Duration::from_millis(650)).await;
    ensure!(
        eval::<String>(
            &broken.page,
            format!("() => localStorage.getItem({})", json!(RECOVERY_KEY))
        )
        .await?
            == invalid_text,
        "Invalid group recovery data was overwritten"
    );
    ensure!(
        text(&broken.page, "body").await?.contains("自動保存停止"),
        "Recovery protection was not explained"
    );
    broken.close(browser).await?;
    checks.push("Migrate legacy v1 files and autosaves under the existing key to v2; prevent overwriting invalid group recovery data");
    Ok(json!({"checks":checks,"page_errors":[]}))
}
