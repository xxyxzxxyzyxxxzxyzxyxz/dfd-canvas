use super::*;
use chromiumoxide::cdp::browser_protocol::{
    input::DispatchMouseEventParams,
    network::{EnableParams, SetCacheDisabledParams},
};

async fn two_frames(page: &Page) -> Result<()> {
    js(
        page,
        "() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
    )
    .await
}

async fn start_frames(page: &Page) -> Result<()> {
    js(
        page,
        r#"() => {
        window.__dfdFrames=[]; window.__dfdRunning=true; window.__dfdPrevious=null;
        const sample=t=>{
            if(!window.__dfdRunning)return;
            if(window.__dfdPrevious!==null)window.__dfdFrames.push(t-window.__dfdPrevious);
            window.__dfdPrevious=t;requestAnimationFrame(sample);
        };requestAnimationFrame(sample);
    }"#,
    )
    .await
}

async fn stop_frames(page: &Page) -> Result<Value> {
    let mut frames: Vec<f64> = eval(
        page,
        "() => {window.__dfdRunning=false;return window.__dfdFrames;}",
    )
    .await?;
    ensure!(!frames.is_empty(), "No rendering frames recorded");
    ensure!(
        frames.iter().all(|v| v.is_finite() && *v >= 0.0),
        "Invalid rendering intervals"
    );
    frames.sort_by(f64::total_cmp);
    let mean = frames.iter().sum::<f64>() / frames.len() as f64;
    let median = if frames.len().is_multiple_of(2) {
        (frames[frames.len() / 2 - 1] + frames[frames.len() / 2]) / 2.0
    } else {
        frames[frames.len() / 2]
    };
    let percentile = ((frames.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(frames.len() - 1);
    let round = |v: f64, digits: i32| {
        let scale = 10_f64.powi(digits);
        (v * scale).round() / scale
    };
    Ok(
        json!({"frames":frames.len(),"mean_ms":round(mean,3),"p50_ms":round(median,3),
        "p95_ms":round(frames[percentile],3),"max_ms":round(frames[frames.len()-1],3),
        "over_25_ms":frames.iter().filter(|v|**v>25.0).count(),"approx_fps":round(1000.0/mean,2)}),
    )
}

pub(super) async fn startup(browser: &Browser, url: &str, output: &Path) -> Result<Value> {
    let mut runs = Vec::new();
    for _ in 0..3 {
        let session = Session::new(browser, output, None).await?;
        let page = &session.page;
        page.execute(EnableParams::default()).await?;
        page.execute(SetCacheDisabledParams::new(true)).await?;
        session.navigate(url).await?;
        two_frames(page).await?;
        let metrics: Value = eval(page,r#"() => ({ready_two_frames_ms:performance.now(),
            navigation:((e)=>({transfer_bytes:e.transferSize,encoded_body_bytes:e.encodedBodySize,
                decoded_body_bytes:e.decodedBodySize,duration_ms:e.duration}))(performance.getEntriesByType('navigation')[0]),
            paints:performance.getEntriesByType('paint').map(e=>({name:e.name,start_ms:e.startTime})),
            resources:performance.getEntriesByType('resource').map(e=>({name:e.name.split('/').pop(),
                transfer_bytes:e.transferSize,encoded_body_bytes:e.encodedBodySize,
                decoded_body_bytes:e.decodedBodySize,duration_ms:e.duration}))})"#).await?;
        errors(page).await?;
        runs.push(metrics);
        session.close(browser).await?;
    }
    Ok(json!(runs))
}

pub(super) async fn performance(
    browser: &Browser,
    url: &str,
    output: &Path,
    count_nodes: usize,
    edge_count: usize,
    grouped: bool,
) -> Result<Value> {
    let session = Session::new(browser, output, None).await?;
    session.navigate(url).await?;
    let page = &session.page;
    let suffix = if grouped { "-groups" } else { "" };
    let fixture_path = output.join(format!("performance-{count_nodes}{suffix}.dfd.json"));
    let document = if grouped {
        serde_json::to_value(crate::fixture::make_grouped_document(
            count_nodes,
            edge_count,
        )?)?
    } else {
        fixture(count_nodes, edge_count)?
    };
    write_json(&fixture_path, &document)?;
    let started = Instant::now();
    load_document(page, &fixture_path).await?;
    wait(
        page,
        &format!("document.querySelectorAll('g[data-node-id]').length === {count_nodes}"),
    )
    .await?;
    two_frames(page).await?;
    let import_ms = (started.elapsed().as_secs_f64() * 100000.0).round() / 100.0;
    ensure!(
        count(page, "g[data-edge-id]").await? == edge_count,
        "Imported edge count differs from fixture"
    );
    js(page,r#"() => {
        window.__dfdNode=document.querySelector('g[data-node-id="n-0099"]');
        window.__dfdAdded=0;window.__dfdRemoved=0;
        window.__dfdObserver=new MutationObserver(records=>{
            for(const r of records){window.__dfdAdded+=r.addedNodes.length;window.__dfdRemoved+=r.removedNodes.length;}
        });window.__dfdObserver.observe(document.querySelector('#canvas'),{childList:true,subtree:true});
    }"#).await?;
    let mut metrics = json!({"nodes":count_nodes,"edges":edge_count,"groups":document["graph"]["groups"].as_array().unwrap().len(),"import_to_two_frames_ms":import_ms});
    let (x, y) = body_center(page, if grouped { "n-0001" } else { "n-0000" }).await?;
    mouse(page, "mouseMoved", (x, y), "none", 0, 0).await?;
    mouse(page, "mousePressed", (x, y), "left", 1, 0).await?;
    start_frames(page).await?;
    let mut last = (x, y);
    for index in 0..120 {
        last = (
            x + (index as f64 / 15.0).sin() * 70.0,
            y + (index as f64 / 15.0).cos() * 25.0,
        );
        mouse(page, "mouseMoved", last, "left", 1, 0).await?;
        sleep(Duration::from_millis(12)).await;
    }
    metrics["drag"] = stop_frames(page).await?;
    mouse(page, "mouseReleased", last, "left", 0, 0).await?;
    metrics["unrelated_node_kept"] = json!(
        eval::<bool>(
            page,
            r#"() => window.__dfdNode===document.querySelector('g[data-node-id="n-0099"]')"#
        )
        .await?
    );
    ensure!(
        metrics["unrelated_node_kept"] == true,
        "Dragging replaced unrelated node DOM"
    );
    metrics["drag_dom_children_added_removed"] =
        eval(page, "() => [window.__dfdAdded,window.__dfdRemoved]").await?;
    ensure!(
        metrics["drag_dom_children_added_removed"] == json!([0, 0]),
        "Dragging added or removed SVG child nodes: {}",
        metrics["drag_dom_children_added_removed"]
    );
    js(page, "() => window.__dfdObserver.disconnect()").await?;

    if grouped {
        // Use the visible beginning of the heading; wide frames may extend offscreen.
        let [x, y, _, _] = bounds(page, "g.group .group-header").await?;
        let start = (x + 80.0, y + 16.0);
        mouse(page, "mousePressed", start, "left", 1, 0).await?;
        start_frames(page).await?;
        for index in 0..120 {
            last = (
                start.0 + (index as f64 / 15.0).sin() * 8.0,
                start.1 + (index as f64 / 15.0).cos() * 8.0,
            );
            mouse(page, "mouseMoved", last, "left", 1, 0).await?;
            sleep(Duration::from_millis(12)).await;
        }
        metrics["group_drag"] = stop_frames(page).await?;
        mouse(page, "mouseReleased", last, "left", 0, 0).await?;
        ensure!(
            count(page, ".error-banner").await? == 0,
            "Grouped performance gesture was rejected"
        );
    }

    let (bx, by) = blank(page).await?;
    mouse(page, "mouseMoved", (bx, by), "none", 0, 0).await?;
    mouse(page, "mousePressed", (bx, by), "middle", 4, 0).await?;
    start_frames(page).await?;
    for index in 0..120 {
        last = (
            bx + (index as f64 / 15.0).sin() * 70.0,
            by + (index as f64 / 15.0).cos() * 25.0,
        );
        mouse(page, "mouseMoved", last, "middle", 4, 0).await?;
        sleep(Duration::from_millis(12)).await;
    }
    metrics["pan"] = stop_frames(page).await?;
    mouse(page, "mouseReleased", last, "middle", 0, 0).await?;
    let point = center(page, "#canvas").await?;
    mouse(page, "mouseMoved", point, "none", 0, 0).await?;
    start_frames(page).await?;
    for index in 0..80 {
        page.execute(serde_json::from_value::<DispatchMouseEventParams>(json!({
            "type":"mouseWheel","x":point.0,"y":point.1,"deltaX":0,"deltaY":if index<40 {-12} else {12}
        }))?).await?;
        sleep(Duration::from_millis(18)).await;
    }
    metrics["zoom"] = stop_frames(page).await?;
    click(page, "#fit").await?;
    screenshot(page, output, &format!("performance-{count_nodes}{suffix}")).await?;
    errors(page).await?;
    session.close(browser).await?;
    Ok(metrics)
}
