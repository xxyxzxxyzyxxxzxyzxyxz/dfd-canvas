//! Serve the compiled DFD Canvas application from a validated static directory.

use std::{
    future::Future,
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use percent_encoding::percent_decode_str;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

/// Build the static application router, rejecting incomplete distribution directories.
///
/// The directory is intended for trusted build output and must contain a readable
/// `index.html`. Missing resources return 404 instead of the application HTML.
pub fn router(directory: impl AsRef<Path>) -> io::Result<Router> {
    let directory = directory.as_ref();
    let root = directory.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot open distribution directory {}: {error}",
                directory.display()
            ),
        )
    })?;
    if !root.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("distribution path is not a directory: {}", root.display()),
        ));
    }
    let index_path = root.join("index.html");
    let index = std::fs::File::open(&index_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot open {}: {error}; build the frontend first",
                index_path.display()
            ),
        )
    })?;
    if !index.metadata()?.is_file() || !index_path.canonicalize()?.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "index.html must be a regular file inside the distribution directory",
        ));
    }

    Ok(Router::new()
        .fallback_service(ServeDir::new(&root))
        .layer(middleware::from_fn_with_state(
            Arc::new(root),
            inside_distribution,
        )))
}

/// Serve on an already-bound listener, stopping gracefully when `shutdown` resolves.
///
/// Passing a listener bound to port zero lets tools run an isolated temporary server.
pub async fn serve(
    listener: TcpListener,
    directory: impl AsRef<Path>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, router(directory)?)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn inside_distribution(
    State(root): State<Arc<PathBuf>>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(decoded) = percent_decode_str(request.uri().path()).decode_utf8() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let relative = Path::new(decoded.trim_start_matches('/'));
    if decoded.contains('\\')
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    // ServeDir rejects '..' itself. Also check canonical targets so a symlink in
    // build output cannot expose files outside the distribution directory.
    if let Ok(mut target) = tokio::fs::canonicalize(root.join(relative)).await {
        if !target.starts_with(root.as_ref()) {
            return StatusCode::NOT_FOUND.into_response();
        }
        if tokio::fs::metadata(&target)
            .await
            .is_ok_and(|metadata| metadata.is_dir())
        {
            target = target.join("index.html");
            if let Ok(index) = tokio::fs::canonicalize(target).await
                && !index.starts_with(root.as_ref())
            {
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    }
    next.run(request).await
}
