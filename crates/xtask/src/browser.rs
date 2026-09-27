//! Chromium acceptance tests and rendering measurements, driven directly from Rust.
//! JavaScript here observes DOM state and injects faults; editing, dragging, file
//! upload, and downloading all use Chromium's real input and file APIs.

mod acceptance;
mod groups;
mod measurements;
mod reconnect;

use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use chromiumoxide::cdp::js_protocol::runtime::EventExceptionThrown;
use chromiumoxide::{
    Browser, BrowserConfig, Page,
    cdp::browser_protocol::{
        browser::{EventDownloadProgress, EventDownloadWillBegin, SetDownloadBehaviorParams},
        dom::SetFileInputFilesParams,
        emulation::SetLocaleOverrideParams,
        input::{DispatchKeyEventParams, DispatchMouseEventParams, InsertTextParams},
        page::{EventJavascriptDialogOpening, HandleJavaScriptDialogParams},
        target::{CreateBrowserContextParams, CreateTargetParams},
    },
    handler::viewport::Viewport,
    page::ScreenshotParams,
};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::{
    sync::Mutex,
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};

const WAIT: Duration = Duration::from_secs(20);
// Keep testing recovery from the legacy key used before the DFD Canvas rename.
const RECOVERY_KEY: &str = "dfd-studio.document.v1";
type Point = (f64, f64);

/// Run the requested suite, always leaving a machine-readable report on failure.
pub async fn run(url: &str, output: &Path, only: &str, headed: bool) -> Result<()> {
    ensure!(
        matches!(only, "all" | "startup" | "acceptance" | "performance"),
        "Unknown suite: {only}"
    );
    std::fs::create_dir_all(output)?;
    let output = output.canonicalize()?;
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|v| v.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into());
    let mut report = json!({
        "os": std::env::consts::OS, "architecture": std::env::consts::ARCH,
        "runner": "Rust / chromiumoxide 0.9.1", "cpu": cpu,
        "headless": !headed, "viewport": "1440x1000", "url": url,
        "method": "Real Chromium DevTools pointer/wheel/key input and actual file upload/download. requestAnimationFrame intervals include headless rendering and automation overhead. Local HTTP, no CPU/network throttle, fresh isolated browser contexts for startup."
    });
    let result = run_browser(url, &output, only, headed, &mut report).await;
    if let Err(error) = &result {
        report["failure"] = json!(format!("{error:#}"));
    }
    write_json(&output.join(format!("report-{only}.json")), &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    result
}

async fn run_browser(
    url: &str,
    output: &Path,
    only: &str,
    headed: bool,
    report: &mut Value,
) -> Result<()> {
    let executable = std::env::var_os("DFD_CHROMIUM").unwrap_or_else(|| "/usr/bin/chromium".into());
    let mut config = BrowserConfig::builder()
        .chrome_executable(executable)
        .arg("--disable-dev-shm-usage")
        .arg("--lang=ja-JP")
        .viewport(Viewport {
            width: 1440,
            height: 1000,
            ..Default::default()
        })
        .request_timeout(WAIT)
        .launch_timeout(WAIT);
    // Only the dedicated tools container opts out of Chromium's namespace sandbox.
    if std::env::var("DFD_CHROMIUM_NO_SANDBOX").as_deref() == Ok("1") {
        config = config.no_sandbox();
    }
    if headed {
        config = config.with_head();
    }
    let (mut browser, mut handler) = Browser::launch(config.build().map_err(|e| anyhow!(e))?)
        .await
        .context("Launching Chromium; set DFD_CHROMIUM to its executable path")?;
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(error) = event {
                eprintln!("Chromium protocol: {error}");
            }
        }
    });
    let result: Result<()> = async {
        report["browser"] = json!(browser.version().await?.product);
        if matches!(only, "all" | "startup") {
            report["startup"] = measurements::startup(&browser, url, output)
                .await
                .context("Startup suite")?;
            println!("Startup measured");
        }
        if matches!(only, "all" | "acceptance") {
            report["acceptance"] = acceptance::acceptance(&browser, url, output)
                .await
                .context("Acceptance suite")?;
            report["robustness"] = acceptance::robustness(&browser, url, output)
                .await
                .context("Robustness suite")?;
            report["groups"] = groups::acceptance(&browser, url, output)
                .await
                .context("Group suite")?;
            report["reconnect"] = reconnect::acceptance(&browser, url, output)
                .await
                .context("Edge endpoint relocation suite")?;
            println!("Acceptance passed");
        }
        if matches!(only, "all" | "performance") {
            report["performance"] = json!([]);
            for (count, edges) in [(100, 200), (500, 1000)] {
                for grouped in [false, true] {
                    let metrics =
                        measurements::performance(&browser, url, output, count, edges, grouped)
                            .await
                            .with_context(|| format!("Performance {count}/{edges}"))?;
                    report["performance"]
                        .as_array_mut()
                        .expect("array initialized above")
                        .push(metrics);
                    println!("Performance {count}/{edges} measured");
                }
            }
        }
        Ok(())
    }
    .await;
    if result.is_err()
        && let Ok(pages) = browser.pages().await
    {
        for (index, page) in pages.iter().enumerate() {
            // Preserve the original error if a disconnected page cannot be captured.
            let _ = timeout(
                Duration::from_secs(5),
                screenshot(page, output, &format!("failure-{index}")),
            )
            .await;
        }
    }
    let _ = timeout(Duration::from_secs(5), browser.close()).await;
    if timeout(Duration::from_secs(5), browser.wait())
        .await
        .is_err()
    {
        let _ = browser.kill().await;
    }
    handler_task.abort();
    result
}

struct Session {
    page: Page,
    context: chromiumoxide::cdp::browser_protocol::browser::BrowserContextId,
    dialog_task: JoinHandle<()>,
    exception_task: JoinHandle<()>,
    exceptions: Arc<Mutex<Vec<String>>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.dialog_task.abort();
        self.exception_task.abort();
    }
}

impl Session {
    async fn new(browser: &Browser, output: &Path, init: Option<&str>) -> Result<Self> {
        let context = browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await?;
        let downloads = output.join(".downloads");
        std::fs::create_dir_all(&downloads)?;
        browser
            .execute(serde_json::from_value::<SetDownloadBehaviorParams>(
                json!({
                    "behavior": "allowAndName", "browserContextId": context,
                    "downloadPath": downloads, "eventsEnabled": true
                }),
            )?)
            .await?;
        let page = browser
            .new_page(serde_json::from_value::<CreateTargetParams>(json!({
                "url": "about:blank", "browserContextId": context
            }))?)
            .await?;
        let mut thrown = page.event_listener::<EventExceptionThrown>().await?;
        let exceptions = Arc::new(Mutex::new(Vec::new()));
        let collected = Arc::clone(&exceptions);
        let exception_task = tokio::spawn(async move {
            while let Some(event) = thrown.next().await {
                collected
                    .lock()
                    .await
                    .push(format!("{:?}", event.exception_details));
            }
        });
        page.execute(serde_json::from_value::<SetLocaleOverrideParams>(
            json!({"locale": "ja-JP"}),
        )?)
        .await?;
        page.add_init_script("window.__dfdErrors = []; window.addEventListener('error', e => window.__dfdErrors.push(e.message)); window.addEventListener('unhandledrejection', e => window.__dfdErrors.push(String(e.reason)));".to_owned()).await?;
        if let Some(init) = init {
            page.add_init_script(init.to_owned()).await?;
        }
        let mut dialogs = page
            .event_listener::<EventJavascriptDialogOpening>()
            .await?;
        let dialog_page = page.clone();
        let dialog_task = tokio::spawn(async move {
            while dialogs.next().await.is_some() {
                if dialog_page
                    .execute(HandleJavaScriptDialogParams::new(true))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            page,
            context,
            dialog_task,
            exception_task,
            exceptions,
        })
    }

    async fn navigate(&self, url: &str) -> Result<()> {
        self.page.goto(url).await?;
        wait(&self.page, "document.querySelector('#canvas') !== null").await?;
        check(
            &self.page,
            "window.innerWidth === 1440 && window.innerHeight === 1000",
            "Browser viewport differs from the reported 1440x1000 test conditions",
        )
        .await
    }

    async fn close(self, browser: &Browser) -> Result<()> {
        // Validate before disposal so the caller can capture a failing page.
        self.errors().await?;
        browser
            .dispose_browser_context(self.context.clone())
            .await?;
        Ok(())
    }

    async fn errors(&self) -> Result<Vec<String>> {
        let mut messages = errors(&self.page).await?;
        messages.extend(self.exceptions.lock().await.iter().cloned());
        ensure!(
            messages.is_empty(),
            "Page exceptions across navigation/reload: {messages:?}"
        );
        Ok(messages)
    }
}

async fn eval<T: DeserializeOwned>(page: &Page, script: impl Into<String>) -> Result<T> {
    Ok(page.evaluate(script.into()).await?.into_value()?)
}

async fn js(page: &Page, script: impl Into<String>) -> Result<()> {
    page.evaluate(script.into()).await?;
    Ok(())
}

async fn wait(page: &Page, expression: &str) -> Result<()> {
    let started = Instant::now();
    loop {
        if eval::<bool>(page, format!("() => Boolean({expression})"))
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }
        ensure!(
            started.elapsed() < WAIT,
            "Timed out waiting for: {expression}"
        );
        sleep(Duration::from_millis(25)).await;
    }
}

async fn check(page: &Page, expression: &str, message: &str) -> Result<()> {
    ensure!(
        eval::<bool>(page, format!("() => Boolean({expression})")).await?,
        "{message}"
    );
    Ok(())
}

async fn count(page: &Page, selector: &str) -> Result<usize> {
    eval(
        page,
        format!(
            "() => document.querySelectorAll({}).length",
            json!(selector)
        ),
    )
    .await
}

async fn text(page: &Page, selector: &str) -> Result<String> {
    eval(
        page,
        format!(
            "() => document.querySelector({}).textContent",
            json!(selector)
        ),
    )
    .await
}

async fn value(page: &Page, selector: &str) -> Result<String> {
    eval(
        page,
        format!("() => document.querySelector({}).value", json!(selector)),
    )
    .await
}

async fn attribute(page: &Page, selector: &str, name: &str) -> Result<String> {
    eval(
        page,
        format!(
            "() => document.querySelector({}).getAttribute({})",
            json!(selector),
            json!(name)
        ),
    )
    .await
}

fn node(id: &str) -> String {
    format!("g[data-node-id={}]", json!(id))
}

async fn bounds(page: &Page, selector: &str) -> Result<[f64; 4]> {
    let rect: [f64; 4] = eval(page, format!("() => {{const r = document.querySelector({}).getBoundingClientRect(); return [r.x,r.y,r.width,r.height];}}", json!(selector))).await?;
    ensure!(
        rect[2] > 0.0 && rect[3] > 0.0,
        "Element has no visible bounding box: {selector}"
    );
    Ok(rect)
}

async fn center(page: &Page, selector: &str) -> Result<Point> {
    let [x, y, width, height] = bounds(page, selector).await?;
    Ok((x + width / 2.0, y + height / 2.0))
}

async fn body_center(page: &Page, id: &str) -> Result<Point> {
    center(page, &format!("{} .node-shape", node(id))).await
}

async fn port(page: &Page, id: &str, side: &str) -> Result<Point> {
    center(page, &format!("{} [data-port={}]", node(id), json!(side))).await
}

async fn blank(page: &Page) -> Result<Point> {
    let [x, y, _, height] = bounds(page, "#canvas").await?;
    Ok((x + 25.0, y + height - 25.0))
}

async fn mouse(
    page: &Page,
    kind: &str,
    point: Point,
    button: &str,
    buttons: u8,
    modifiers: u8,
) -> Result<()> {
    page.execute(serde_json::from_value::<DispatchMouseEventParams>(json!({
        "type": kind, "x": point.0, "y": point.1, "button": button,
        "buttons": buttons, "modifiers": modifiers,
        "clickCount": if matches!(kind, "mousePressed" | "mouseReleased") { 1 } else { 0 }
    }))?)
    .await?;
    Ok(())
}

async fn click_at(page: &Page, point: Point, modifiers: u8) -> Result<()> {
    mouse(page, "mouseMoved", point, "none", 0, modifiers).await?;
    mouse(page, "mousePressed", point, "left", 1, modifiers).await?;
    mouse(page, "mouseReleased", point, "left", 0, modifiers).await
}

async fn click(page: &Page, selector: &str) -> Result<()> {
    click_at(page, center(page, selector).await?, 0)
        .await
        .with_context(|| format!("Clicking {selector}"))
}

async fn drag(page: &Page, source: Point, target: Point, steps: usize) -> Result<()> {
    mouse(page, "mouseMoved", source, "none", 0, 0).await?;
    mouse(page, "mousePressed", source, "left", 1, 0).await?;
    for step in 1..=steps {
        let fraction = step as f64 / steps as f64;
        mouse(
            page,
            "mouseMoved",
            (
                source.0 + (target.0 - source.0) * fraction,
                source.1 + (target.1 - source.1) * fraction,
            ),
            "left",
            1,
            0,
        )
        .await?;
    }
    mouse(page, "mouseReleased", target, "left", 0, 0).await
}

async fn key(page: &Page, key: &str, down: bool, modifiers: u8) -> Result<()> {
    let (code, virtual_code) = match key {
        "Tab" => ("Tab", 9),
        "Escape" => ("Escape", 27),
        "Backspace" => ("Backspace", 8),
        "Delete" => ("Delete", 46),
        "Shift" => ("ShiftLeft", 16),
        "Control" => ("ControlLeft", 17),
        "a" => ("KeyA", 65),
        "z" => ("KeyZ", 90),
        _ => bail!("Unsupported test key: {key}"),
    };
    page.execute(serde_json::from_value::<DispatchKeyEventParams>(json!({
        "type": if down { "rawKeyDown" } else { "keyUp" }, "key": key,
        "code": code, "windowsVirtualKeyCode": virtual_code, "modifiers": modifiers
    }))?)
    .await?;
    Ok(())
}

async fn press(page: &Page, name: &str, modifiers: u8) -> Result<()> {
    key(page, name, true, modifiers).await?;
    key(page, name, false, modifiers).await
}

async fn focus(page: &Page, selector: &str) -> Result<()> {
    page.find_element(selector).await?.focus().await?;
    Ok(())
}

async fn commit_field(page: &Page, selector: &str, text: &str) -> Result<()> {
    focus(page, selector).await?;
    press(page, "a", 2).await?;
    page.execute(InsertTextParams::new(text)).await?;
    press(page, "Tab", 0).await?;
    Ok(())
}

async fn load_document(page: &Page, path: &Path) -> Result<()> {
    let input = page.find_element("#file-input").await?;
    page.execute(serde_json::from_value::<SetFileInputFilesParams>(json!({
        "files": [path.canonicalize()?], "backendNodeId": input.backend_node_id
    }))?)
    .await?;
    sleep(Duration::from_millis(100)).await;
    Ok(())
}

async fn download(
    browser: &Browser,
    page: &Page,
    output: &Path,
    selector: &str,
    name: &str,
    suffix: &str,
) -> Result<Value> {
    let mut starts = browser.event_listener::<EventDownloadWillBegin>().await?;
    let mut progress = browser.event_listener::<EventDownloadProgress>().await?;
    click(page, selector).await?;
    let started = timeout(WAIT, starts.next())
        .await
        .context("Download did not start")?
        .context("Download event stream ended")?;
    ensure!(
        started.suggested_filename.ends_with(suffix),
        "Unexpected download name: {}",
        started.suggested_filename
    );
    timeout(WAIT, async {
        while let Some(event) = progress.next().await {
            if event.guid == started.guid {
                match event.state.as_ref() {
                    "completed" => return Ok(()),
                    "canceled" => bail!("Download canceled: {}", started.suggested_filename),
                    _ => {}
                }
            }
        }
        bail!("Download progress stream ended")
    })
    .await
    .context("Download did not complete")??;
    let destination = output.join(name);
    std::fs::rename(output.join(".downloads").join(&started.guid), &destination).with_context(
        || {
            format!(
                "Keeping actual Chromium download at {}",
                destination.display()
            )
        },
    )?;
    Ok(serde_json::from_slice(&std::fs::read(&destination)?)?)
}

async fn save_document(browser: &Browser, page: &Page, output: &Path, name: &str) -> Result<Value> {
    download(
        browser,
        page,
        output,
        "#save",
        &format!("{name}.dfd.json"),
        ".dfd.json",
    )
    .await
    .with_context(|| format!("Saving document {name}"))
}

fn graph_equal(a: &Value, b: &Value) -> Result<()> {
    ensure!(a["name"] == b["name"], "Document names differ");
    for category in ["nodes", "edges", "groups"] {
        let mut left = a["graph"][category]
            .as_array()
            .context("Missing graph array")?
            .clone();
        let mut right = b["graph"][category]
            .as_array()
            .context("Missing graph array")?
            .clone();
        left.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_owned());
        right.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_owned());
        ensure!(left == right, "Semantic {category} changed");
    }
    Ok(())
}

fn fixture(nodes: usize, edges: usize) -> Result<Value> {
    Ok(serde_json::to_value(crate::fixture::make_document(
        nodes, edges,
    )?)?)
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

async fn screenshot(page: &Page, output: &Path, name: &str) -> Result<()> {
    let viewport: [u32; 2] = eval(page, "() => [innerWidth, innerHeight]").await?;
    // The editor fills the viewport. chromiumoxide 0.9.1's full-page helper
    // clears device metrics afterward, losing the configured test viewport.
    page.save_screenshot(
        ScreenshotParams::default(),
        output.join(format!("{name}.png")),
    )
    .await?;
    ensure!(
        eval::<[u32; 2]>(page, "() => [innerWidth, innerHeight]").await? == viewport,
        "Screenshot changed the browser viewport"
    );
    Ok(())
}

async fn errors(page: &Page) -> Result<Vec<String>> {
    let errors: Vec<String> = eval(page, "() => window.__dfdErrors").await?;
    ensure!(errors.is_empty(), "Page errors: {errors:?}");
    Ok(errors)
}
