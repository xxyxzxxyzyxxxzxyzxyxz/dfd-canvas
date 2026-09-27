use std::{fs, time::Duration};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time::timeout,
};
use tower::ServiceExt;

fn distribution() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>DFD</title>",
    )
    .unwrap();
    fs::write(directory.path().join("app.wasm"), b"\0asm\x01\0\0\0").unwrap();
    fs::write(directory.path().join("style.css"), "body { margin: 0; }").unwrap();
    directory
}

#[tokio::test]
async fn serves_html_and_wasm_with_head_metadata() {
    let directory = distribution();
    let app = dfd_server::router(directory.path()).unwrap();
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html");
    assert!(
        to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .starts_with(b"<!doctype html>")
    );

    for method in ["GET", "HEAD"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/app.wasm")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/wasm");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "8");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(
            body.as_ref(),
            if method == "GET" {
                &b"\0asm\x01\0\0\0"[..]
            } else {
                &[]
            }
        );
    }
}

#[tokio::test]
async fn rejects_missing_paths_traversal_and_unsupported_methods() {
    let directory = distribution();
    let app = dfd_server::router(directory.path()).unwrap();
    for path in [
        "/missing.js",
        "/unknown-route",
        "/../index.html",
        "/%2e%2e/index.html",
        "/%2e%2e%2findex.html",
        "/%5cindex.html",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/index.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinks_outside_distribution() {
    use std::os::unix::fs::symlink;

    let directory = distribution();
    let outside = distribution();
    symlink(
        outside.path().join("index.html"),
        directory.path().join("secret.html"),
    )
    .unwrap();
    symlink(outside.path(), directory.path().join("outside")).unwrap();
    fs::create_dir(directory.path().join("nested")).unwrap();
    symlink(
        outside.path().join("index.html"),
        directory.path().join("nested/index.html"),
    )
    .unwrap();
    let app = dfd_server::router(directory.path()).unwrap();
    for path in ["/secret.html", "/outside/index.html", "/nested/"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[test]
fn startup_rejects_missing_or_invalid_distribution() {
    let directory = tempfile::tempdir().unwrap();
    assert!(dfd_server::router(directory.path().join("missing")).is_err());
    assert!(dfd_server::router(directory.path()).is_err());
    fs::create_dir(directory.path().join("index.html")).unwrap();
    assert!(dfd_server::router(directory.path()).is_err());

    let valid = distribution();
    assert!(dfd_server::router(valid.path().join("index.html")).is_err());
}

#[test]
fn binary_reports_invalid_directory_and_busy_port() {
    let empty = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dfd-server"))
        .arg("--dir")
        .arg(empty.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("build the frontend first"));

    let directory = distribution();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dfd-server"))
        .arg("--dir")
        .arg(directory.path())
        .arg("--listen")
        .arg(listener.local_addr().unwrap().to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot listen on"));
}

#[tokio::test]
async fn listener_serves_requests_and_stops_gracefully() {
    let directory = distribution();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let path = directory.path().to_owned();
    let server = tokio::spawn(async move {
        dfd_server::serve(listener, path, async {
            let _ = stopped.await;
        })
        .await
    });
    let mut client = TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    timeout(Duration::from_secs(5), client.read_to_string(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("<title>DFD</title>"));
    stop.send(()).unwrap();
    timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect(address).await.is_err());
}
