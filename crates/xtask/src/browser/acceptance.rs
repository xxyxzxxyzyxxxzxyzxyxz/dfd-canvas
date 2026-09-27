use super::*;

pub(super) async fn acceptance(browser: &Browser, url: &str, output: &Path) -> Result<Value> {
    let session = Session::new(browser, output, None).await?;
    session.navigate(url).await?;
    let page = &session.page;
    ensure!(
        count(page, "g[data-node-id]").await? == 0,
        "Initial document must be empty"
    );
    check(
        page,
        "document.querySelector('#undo').disabled",
        "Undo must initially be disabled",
    )
    .await?;
    let mut checks = Vec::new();
    let mut ids = Vec::new();
    for (index, kind) in ["process", "store", "entity"].iter().enumerate() {
        let before: BTreeSet<String> = eval(
            page,
            "() => [...document.querySelectorAll('g[data-node-id]')].map(e => e.dataset.nodeId)",
        )
        .await?;
        click(page, &format!("#add-{kind}")).await?;
        let after: BTreeSet<String> = eval(
            page,
            "() => [...document.querySelectorAll('g[data-node-id]')].map(e => e.dataset.nodeId)",
        )
        .await?;
        let added: Vec<_> = after.difference(&before).cloned().collect();
        ensure!(added.len() == 1, "Adding {kind} should create one node");
        let id = added[0].clone();
        let source = body_center(page, &id).await?;
        let [x, y, _, _] = bounds(page, "#canvas").await?;
        drag(
            page,
            source,
            (
                x + 180.0 + index as f64 * 300.0,
                y + 180.0 + (index % 2) as f64 * 180.0,
            ),
            20,
        )
        .await?;
        commit_field(
            page,
            "#prop-name",
            ["注文を検証する", "注文データ", "利用者"][index],
        )
        .await?;
        commit_field(
            page,
            "#prop-description",
            &format!("日本語の説明 {}：受注業務で利用します。", index + 1),
        )
        .await?;
        ids.push(id);
    }
    checks
        .push("Add all three node types to an empty canvas; edit Japanese names and descriptions");

    drag(
        page,
        port(page, &ids[2], "left").await?,
        port(page, &ids[0], "top").await?,
        20,
    )
    .await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 1,
        "First edge missing"
    );
    commit_field(page, "#prop-name", "注文情報").await?;
    commit_field(page, "#prop-description", "利用者から届く注文の明細").await?;
    drag(
        page,
        port(page, &ids[0], "right").await?,
        port(page, &ids[1], "left").await?,
        20,
    )
    .await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 2,
        "Second edge missing"
    );
    commit_field(page, "#prop-name", "検証済み注文").await?;
    checks.push("Create two directed edges by dragging connection ports; use Japanese labels");

    let baseline = save_document(browser, page, output, "initial").await?;
    let kinds: BTreeSet<_> = baseline["graph"]["nodes"]
        .as_array()
        .context("nodes array")?
        .iter()
        .map(|n| n["kind"].as_str().unwrap_or_default())
        .collect();
    ensure!(
        kinds == BTreeSet::from(["process", "data_store", "external_entity"]),
        "Incorrect node kinds"
    );
    let endpoints: BTreeSet<_> = baseline["graph"]["edges"]
        .as_array()
        .context("edges array")?
        .iter()
        .map(|e| {
            (
                e["source"].as_str().unwrap_or_default(),
                e["target"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    ensure!(
        endpoints
            == BTreeSet::from([
                (ids[2].as_str(), ids[0].as_str()),
                (ids[0].as_str(), ids[1].as_str())
            ]),
        "Incorrect directed endpoints"
    );
    let point: [f64; 2] = eval(page, "() => {const e=document.querySelector('g[data-edge-id] .edge-hit'); const p=e.getPointAtLength(e.getTotalLength()*.32).matrixTransform(e.getScreenCTM()); return [p.x,p.y];}").await?;
    click_at(page, (point[0], point[1]), 0).await?;
    ensure!(
        count(page, "g[data-edge-id].selected").await? == 1,
        "Edge hit target did not select edge"
    );
    // Deselect so the node's port starts a new edge instead of moving the
    // selected edge's endpoint handle.
    click_at(page, blank(page).await?, 0).await?;
    drag(
        page,
        port(page, &ids[0], "top").await?,
        blank(page).await?,
        20,
    )
    .await?;
    drag(
        page,
        port(page, &ids[0], "left").await?,
        port(page, &ids[0], "bottom").await?,
        20,
    )
    .await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 2,
        "Blank/self connection added an edge"
    );
    let start = port(page, &ids[0], "right").await?;
    let end = blank(page).await?;
    mouse(page, "mouseMoved", start, "none", 0, 0).await?;
    mouse(page, "mousePressed", start, "left", 1, 0).await?;
    mouse(page, "mouseMoved", end, "left", 1, 0).await?;
    press(page, "Escape", 0).await?;
    mouse(page, "mouseReleased", end, "left", 0, 0).await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 2,
        "Esc left an unconnected edge"
    );
    checks.push("Leave no dangling edges after dropping on empty space, attempting a self-connection, or canceling with Esc");

    let before_cancel = save_document(browser, page, output, "before-cancel").await?;
    for cancellation in ["pointercancel", "blur"] {
        let (x, y) = body_center(page, &ids[0]).await?;
        mouse(page, "mouseMoved", (x, y), "none", 0, 0).await?;
        mouse(page, "mousePressed", (x, y), "left", 1, 0).await?;
        mouse(page, "mouseMoved", (x + 40.0, y + 20.0), "left", 1, 0).await?;
        // Only cancellation is synthetic; the drag itself uses real pointer input.
        if cancellation == "pointercancel" {
            js(page, "() => document.querySelector('#canvas').dispatchEvent(new PointerEvent('pointercancel', {pointerId:1, bubbles:true}))").await?;
        } else {
            js(page, "() => window.dispatchEvent(new Event('blur'))").await?;
        }
        mouse(page, "mouseReleased", (x + 40.0, y + 20.0), "left", 0, 0).await?;
        ensure!(
            save_document(browser, page, output, &format!("cancel-{cancellation}")).await?
                == before_cancel,
            "{cancellation} failed to roll back drag"
        );
    }
    checks.push("Roll back dragging on pointercancel or loss of focus");

    click(page, "#ai-export").await?;
    let ai_before = value(page, "#ai-json").await?;
    let parsed_ai: Value = serde_json::from_str(&ai_before)?;
    ensure!(
        parsed_ai.get("layout").is_none(),
        "AI JSON must omit layout"
    );
    graph_equal(&baseline, &parsed_ai)?;
    click(page, "#ai-close").await?;

    let (bx, by) = blank(page).await?;
    drag(page, (bx, by), (bx + 65.0, by - 35.0), 20).await?;
    click(page, "#zoom-in").await?;
    let old_path = attribute(page, "g[data-edge-id] .edge-path", "d").await?;
    click_at(page, body_center(page, &ids[0]).await?, 0).await?;
    let previous = save_document(browser, page, output, "before-drag").await?;
    let (x, y) = body_center(page, &ids[0]).await?;
    drag(page, (x, y), (x + 70.0, y + 40.0), 35).await?;
    let moved = save_document(browser, page, output, "after-drag").await?;
    ensure!(
        moved["layout"]["nodes"][&ids[0]]["x"] != previous["layout"]["nodes"][&ids[0]]["x"],
        "Node did not move after zoom/pan"
    );
    ensure!(
        attribute(page, "g[data-edge-id] .edge-path", "d").await? != old_path,
        "Connected edge did not follow node"
    );
    click(page, "#undo").await?;
    let undone = save_document(browser, page, output, "drag-undone").await?;
    ensure!(
        undone["layout"]["nodes"] == previous["layout"]["nodes"],
        "One Undo did not undo complete drag"
    );
    click(page, "#redo").await?;
    click_at(page, body_center(page, &ids[0]).await?, 0).await?;
    let (x, y) = center(page, &format!("{} .resize-handle", node(&ids[0]))).await?;
    drag(page, (x, y), (x + 65.0, y + 30.0), 20).await?;
    let resized = save_document(browser, page, output, "resized").await?;
    ensure!(
        resized["layout"]["nodes"][&ids[0]]["width"].as_f64()
            > moved["layout"]["nodes"][&ids[0]]["width"].as_f64(),
        "Resize did not enlarge width"
    );
    graph_equal(&resized, &baseline)?;
    click(page, "#ai-export").await?;
    ensure!(
        value(page, "#ai-json").await? == ai_before,
        "Layout changes altered AI JSON"
    );
    click(page, "#ai-close").await?;
    checks.push("Move and resize after panning and zooming; connected edges follow; undo each drag in one step; preserve AI JSON");

    click_at(page, body_center(page, &ids[0]).await?, 0).await?;
    key(page, "Shift", true, 8).await?;
    click_at(page, body_center(page, &ids[1]).await?, 8).await?;
    key(page, "Shift", false, 0).await?;
    let group_before = save_document(browser, page, output, "group-before").await?;
    let (x, y) = body_center(page, &ids[0]).await?;
    drag(page, (x, y), (x + 30.0, y - 20.0), 20).await?;
    let group_after = save_document(browser, page, output, "group-after").await?;
    let deltas: Vec<f64> = ids[..2]
        .iter()
        .map(|id| {
            group_after["layout"]["nodes"][id]["x"]
                .as_f64()
                .unwrap_or_default()
                - group_before["layout"]["nodes"][id]["x"]
                    .as_f64()
                    .unwrap_or_default()
        })
        .collect();
    ensure!(
        deltas[0] > 0.0 && (deltas[0] - deltas[1]).abs() < 1e-8,
        "Shift selection did not move both nodes equally"
    );
    click(page, "#undo").await?;
    let group_undone = save_document(browser, page, output, "group-undone").await?;
    ensure!(
        group_undone["layout"]["nodes"] == group_before["layout"]["nodes"],
        "Group drag Undo failed"
    );
    checks.push("Select multiple nodes with Shift; move them together and undo in one step");

    click_at(page, blank(page).await?, 0).await?;
    click_at(page, body_center(page, &ids[0]).await?, 0).await?;
    focus(page, "#canvas").await?;
    js(page, "() => document.querySelector('#canvas').dispatchEvent(new KeyboardEvent('keydown', {key:'Delete',isComposing:true,bubbles:true}))").await?;
    ensure!(
        count(page, "g[data-node-id]").await? == 3,
        "Composing Delete removed a diagram node"
    );
    focus(page, "#prop-name").await?;
    press(page, "Backspace", 0).await?;
    press(page, "z", 2).await?;
    ensure!(
        count(page, "g[data-node-id]").await? == 3,
        "Text shortcut affected diagram"
    );
    commit_field(page, "#prop-name", "注文を検証する").await?;
    let before_delete = save_document(browser, page, output, "before-delete").await?;
    click(page, "#delete").await?;
    ensure!(
        count(page, "g[data-node-id]").await? == 2,
        "Delete did not remove selected node"
    );
    ensure!(
        count(page, "g[data-edge-id]").await? == 0,
        "Delete left incident edges"
    );
    click(page, "#undo").await?;
    ensure!(
        save_document(browser, page, output, "delete-undone").await? == before_delete,
        "Undo did not restore IDs and content"
    );
    click(page, "#redo").await?;
    ensure!(
        count(page, "g[data-node-id]").await? == 2,
        "Delete Redo failed"
    );
    click(page, "#undo").await?;
    checks.push("Protect text inputs from editor shortcuts; delete connected nodes and undo/redo with the same IDs");

    let saved = save_document(browser, page, output, "roundtrip").await?;
    click(page, "#new").await?;
    ensure!(
        count(page, "g[data-node-id]").await? == 0,
        "New did not clear document"
    );
    load_document(page, &output.join("roundtrip.dfd.json")).await?;
    ensure!(
        save_document(browser, page, output, "reloaded").await? == saved,
        "File roundtrip changed document"
    );
    let invalid = output.join("invalid.dfd.json");
    std::fs::write(
        &invalid,
        include_str!("../../../../tests/fixtures/invalid.dfd.json"),
    )?;
    load_document(page, &invalid).await?;
    ensure!(
        save_document(browser, page, output, "after-invalid").await? == saved,
        "Invalid input replaced document"
    );
    ensure!(
        text(page, "body").await?.contains("schema_version"),
        "Invalid schema error not shown"
    );
    checks.push("Fully restore graph semantics and layout when saving and reloading JSON; preserve the document when loading invalid input");

    click(page, "#fit").await?;
    screenshot(page, output, "acceptance").await?;
    click(page, "#ai-export").await?;
    let final_ai: Value = serde_json::from_str(&value(page, "#ai-json").await?)?;
    graph_equal(&final_ai, &saved)?;
    js(page, "() => Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:()=>Promise.reject(new Error('test denied'))}})").await?;
    click(page, "#ai-copy").await?;
    wait(
        page,
        "document.querySelector('.json-dialog').textContent.includes('コピーできませんでした')",
    )
    .await?;
    click(page, "#ai-select").await?;
    check(page, "(() => {const e=document.querySelector('#ai-json');return e.selectionEnd-e.selectionStart===e.value.length;})()", "AI JSON was not completely selected").await?;
    ensure!(
        download(browser, page, output, "#ai-save", "semantic.json", ".json").await? == final_ai,
        "Semantic download changed JSON"
    );
    screenshot(page, output, "ai-json").await?;
    errors(page).await?;
    sleep(Duration::from_millis(1100)).await;
    page.reload().await?;
    wait(page, "document.querySelector('#canvas') !== null").await?;
    let recovered = save_document(browser, page, output, "recovered").await?;
    graph_equal(&recovered, &saved)?;
    checks.push("Display and extract AI JSON; recover from browser autosave");

    click_at(page, body_center(page, &ids[0]).await?, 0).await?;
    let long_name =
        "注文内容を検証して在庫および配送先と支払い方法を確認する非常に長い日本語の処理名";
    commit_field(page, "#prop-name", long_name).await?;
    commit_field(page, "#prop-description", "長い名前の省略表示、選択状態、右側プロパティの全文表示を確認します。\nこの説明はプレーンテキストです。").await?;
    ensure!(
        text(page, &format!("{} .node-name", node(&ids[0])))
            .await?
            .ends_with('…'),
        "Long node name is not ellipsized"
    );
    ensure!(
        attribute(page, &node(&ids[0]), "class")
            .await?
            .contains("selected"),
        "Edited node lost selection"
    );
    ensure!(
        value(page, "#prop-name").await? == long_name,
        "Properties do not retain complete long name"
    );
    screenshot(page, output, "selected-long-name").await?;
    checks.push(
        "Truncate long Japanese names; preserve selection and show the full text in properties",
    );
    let page_errors = session.errors().await?;
    session.close(browser).await?;
    Ok(json!({"passed":true, "checks":checks, "page_errors":page_errors}))
}

pub(super) async fn robustness(browser: &Browser, url: &str, output: &Path) -> Result<Value> {
    let mut checks = Vec::new();
    let session = Session::new(browser, output, Some("window.__dfdWrites=0; const original=Storage.prototype.setItem; Storage.prototype.setItem=function(...args){window.__dfdWrites++;return original.apply(this,args);};")).await?;
    session.navigate(url).await?;
    let page = &session.page;
    click(page, "#add-process").await?;
    let id = attribute(page, "g[data-node-id]", "data-node-id").await?;
    let (x, y) = body_center(page, &id).await?;
    mouse(page, "mouseMoved", (x, y), "none", 0, 0).await?;
    mouse(page, "mousePressed", (x, y), "left", 1, 0).await?;
    let writes: u64 = eval(page, "window.__dfdWrites").await?;
    for index in 0..20 {
        mouse(
            page,
            "mouseMoved",
            (x + index as f64, y + 10.0),
            "left",
            1,
            0,
        )
        .await?;
        sleep(Duration::from_millis(40)).await;
    }
    ensure!(
        eval::<u64>(page, "window.__dfdWrites").await? == writes,
        "LocalStorage write during drag"
    );
    mouse(page, "mouseReleased", (x + 19.0, y + 10.0), "left", 0, 0).await?;
    sleep(Duration::from_millis(800)).await;
    ensure!(
        eval::<u64>(page, "window.__dfdWrites").await? == writes + 1,
        "Drag did not produce exactly one recovery write"
    );
    checks.push("Defer autosave writes during dragging even with a pending timer; write once after committing");

    commit_field(page, "#prop-name", "復旧保存の独立性").await?;
    save_document(browser, page, output, "pending-recovery-save").await?;
    let (x, y) = body_center(page, &id).await?;
    mouse(page, "mouseMoved", (x, y), "none", 0, 0).await?;
    mouse(page, "mousePressed", (x, y), "left", 1, 0).await?;
    mouse(page, "mouseMoved", (x + 25.0, y + 10.0), "left", 1, 0).await?;
    press(page, "Escape", 0).await?;
    mouse(page, "mouseReleased", (x + 25.0, y + 10.0), "left", 0, 0).await?;
    sleep(Duration::from_millis(800)).await;
    let stored: String = eval(
        page,
        format!("localStorage.getItem({})", json!(RECOVERY_KEY)),
    )
    .await?;
    let recovered: Value = serde_json::from_str(&stored)?;
    ensure!(
        recovered["graph"]["nodes"][0]["name"] == "復旧保存の独立性",
        "Cancel lost pending recovery save after file export"
    );
    checks.push("Preserve pending recovery saves when canceling a drag after saving JSON");

    let mut hostile = fixture(3, 2)?;
    hostile["graph"]["nodes"][0]["name"] = json!("<img src=x onerror=\"window.injected=true\">");
    hostile["graph"]["nodes"][0]["description"] = json!("<script>window.injected=true</script>");
    let fixture_path = output.join("text-safety.dfd.json");
    write_json(&fixture_path, &hostile)?;
    load_document(page, &fixture_path).await?;
    check(
        page,
        "window.injected === undefined",
        "Imported string executed as code",
    )
    .await?;
    ensure!(
        count(page, "svg img, svg script").await? == 0,
        "Imported string produced HTML elements"
    );
    ensure!(
        attribute(page, &format!("{} .node-shape", node("n-0000")), "rx").await? == "14",
        "Original node kind rendered incorrectly"
    );
    hostile["graph"]["nodes"][0]["kind"] = json!("data_store");
    hostile["graph"]["edges"][0]["target"] = json!("n-0002");
    write_json(&fixture_path, &hostile)?;
    load_document(page, &fixture_path).await?;
    ensure!(
        count(page, &format!("{} .store-lines", node("n-0000"))).await? == 1,
        "Same-ID kind replacement did not update DOM"
    );
    ensure!(
        attribute(page, &format!("{} .node-shape", node("n-0000")), "rx").await? == "0",
        "Same-ID kind replacement retained process shape"
    );
    ensure!(
        save_document(browser, page, output, "same-id-replaced").await?["graph"]
            == hostile["graph"],
        "Same-ID replacement changed imported graph"
    );
    checks.push("Do not execute loaded strings; redraw loaded objects whose types or connections change while retaining the same IDs");

    drag(
        page,
        port(page, "n-0000", "right").await?,
        port(page, "n-0002", "left").await?,
        20,
    )
    .await?;
    click_at(page, blank(page).await?, 0).await?;
    drag(
        page,
        port(page, "n-0002", "left").await?,
        port(page, "n-0000", "right").await?,
        20,
    )
    .await?;
    ensure!(
        count(page, "g[data-edge-id]").await? == 4,
        "Parallel/reverse edge missing"
    );
    let paths: BTreeSet<String> = eval(page,"() => [...document.querySelectorAll('g[data-edge-id] .edge-path')].map(e=>e.getAttribute('d'))").await?;
    ensure!(
        paths.len() == 4,
        "Parallel/reverse edges must use separate curves"
    );
    click(page, "#undo").await?;
    check(
        page,
        "!document.querySelector('#redo').disabled",
        "Redo unavailable after Undo",
    )
    .await?;
    click(page, "#add-process").await?;
    check(
        page,
        "document.querySelector('#redo').disabled",
        "New edit did not discard Redo",
    )
    .await?;
    checks.push("Keep parallel and reverse edges as distinct IDs and curves; discard redo history on a new edit after undo");

    let mut fitted = fixture(3, 2)?;
    fitted["layout"]["edges"]["e-0000"] = json!({"source_port":"top","target_port":"top"});
    fitted["layout"]["edges"]["e-0001"] = json!({"source_port":"bottom","target_port":"bottom"});
    write_json(&fixture_path, &fitted)?;
    load_document(page, &fixture_path).await?;
    click(page, "#fit").await?;
    check(page,"(() => {const c=document.querySelector('#canvas').getBoundingClientRect();return [...document.querySelectorAll('.node-shape,.edge-path,.edge-label-bg')].every(e=>{const r=e.getBoundingClientRect();return r.left>=c.left&&r.right<=c.right&&r.top>=c.top&&r.bottom<=c.bottom;});})()","Fit must include curved edges and labels, not just nodes").await?;
    screenshot(page, output, "fit-bounds").await?;
    checks.push("Fit the entire diagram within the canvas, including curves and labels between top ports or between bottom ports");
    errors(page).await?;
    session.close(browser).await?;

    let session = Session::new(
        browser,
        output,
        Some(&format!(
            "localStorage.setItem({}, '{{broken recovery');",
            json!(RECOVERY_KEY)
        )),
    )
    .await?;
    session.navigate(url).await?;
    let page = &session.page;
    ensure!(
        count(page, "[role=alert]").await? == 1,
        "Corrupt recovery did not show error"
    );
    click(page, "#add-process").await?;
    sleep(Duration::from_millis(800)).await;
    ensure!(
        eval::<String>(
            page,
            format!("localStorage.getItem({})", json!(RECOVERY_KEY))
        )
        .await?
            == "{broken recovery",
        "Corrupt recovery was overwritten"
    );
    ensure!(
        save_document(browser, page, output, "corrupt-recovery-export").await?["graph"]["nodes"]
            .as_array()
            .context("nodes array")?
            .len()
            == 1,
        "Cannot export after corrupt recovery"
    );
    checks.push(
        "Show errors without deleting or overwriting corrupt recovery data; allow JSON export",
    );
    errors(page).await?;
    session.close(browser).await?;

    let session = Session::new(browser,output,Some("Storage.prototype.setItem=()=>{throw new DOMException('test full','QuotaExceededError');};")).await?;
    session.navigate(url).await?;
    let page = &session.page;
    click(page, "#add-process").await?;
    sleep(Duration::from_millis(800)).await;
    ensure!(
        text(page, "body").await?.contains("自動保存に失敗"),
        "Quota error was reported as successful save"
    );
    ensure!(
        save_document(browser, page, output, "storage-denied-export").await?["graph"]["nodes"]
            .as_array()
            .context("nodes array")?
            .len()
            == 1,
        "Cannot export after storage denial"
    );
    checks.push("Report LocalStorage quota errors as failures; allow JSON export");
    errors(page).await?;
    session.close(browser).await?;
    Ok(json!({"passed":true,"checks":checks}))
}
