use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::{fs, path::PathBuf, process::Command};
use tokio::{net::TcpListener, sync::oneshot};

#[derive(Debug, Parser)]
#[command(about = "DFD Canvas container build and verification tools")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Check formatting, native and Wasm code, tests, and release output.
    Check,
    /// Generate a reproducible performance document.
    GenerateFixture {
        #[arg(value_parser = clap::value_parser!(u32).range(2..))]
        nodes: u32,
        edges: u32,
        output: PathBuf,
    },
    /// Report each release file's raw and independently gzipped size.
    MeasureDist {
        #[arg(env = "DFD_DIST", default_value = "/app/dist")]
        directory: PathBuf,
    },
    /// Run browser checks, starting a temporary Rust server unless --url is supplied.
    BrowserTest {
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "/results")]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = Suite::All)]
        only: Suite,
        #[arg(long)]
        headed: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Suite {
    Acceptance,
    Performance,
    Startup,
    All,
}

impl Suite {
    fn as_str(self) -> &'static str {
        match self {
            Self::Acceptance => "acceptance",
            Self::Performance => "performance",
            Self::Startup => "startup",
            Self::All => "all",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Task::Check => check(),
        Task::GenerateFixture {
            nodes,
            edges,
            output,
        } => {
            let document = xtask::fixture::make_document(nodes as usize, edges as usize)?;
            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            let contents = serde_json::to_string_pretty(&document)? + "\n";
            fs::write(&output, contents)
                .with_context(|| format!("Cannot write fixture {}", output.display()))
        }
        Task::MeasureDist { directory } => {
            let report = xtask::measure::measure_dist(&directory)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Task::BrowserTest {
            url,
            output,
            only,
            headed,
        } => {
            if let Some(url) = url {
                return xtask::browser::run(&url, &output, only.as_str(), headed).await;
            }
            let directory = std::env::var_os("DFD_DIST")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("dist"));
            // Validate before launching a task, so a broken dist fails immediately.
            let _ = dfd_server::router(&directory)?;
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let url = format!("http://{}", listener.local_addr()?);
            let (shutdown, stopped) = oneshot::channel::<()>();
            let server = tokio::spawn(async move {
                dfd_server::serve(listener, directory, async move {
                    let _ = stopped.await;
                })
                .await
            });
            let result = xtask::browser::run(&url, &output, only.as_str(), headed).await;
            let _ = shutdown.send(());
            let server_result = server.await.context("Browser test server task failed")?;
            result?;
            server_result.context("Browser test server failed")
        }
    }
}

fn check() -> Result<()> {
    let current = std::env::current_dir()?;
    let workspace = current
        .ancestors()
        .find(|path| path.join("Trunk.toml").is_file() && path.join("Cargo.toml").is_file())
        .context("Run check from the DFD Canvas workspace")?;
    let commands: &[(&str, &[&str])] = &[
        ("cargo", &["fmt", "--all", "--", "--check"]),
        ("cargo", &["test", "--locked", "--workspace"]),
        (
            "cargo",
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            "cargo",
            &[
                "check",
                "--locked",
                "-p",
                "dfd-canvas",
                "--target",
                "wasm32-unknown-unknown",
            ],
        ),
        (
            "cargo",
            &[
                "clippy",
                "--locked",
                "-p",
                "dfd-canvas",
                "--target",
                "wasm32-unknown-unknown",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ];
    for (program, args) in commands {
        run_command(Command::new(program).args(*args).current_dir(workspace))?;
    }
    let dist = std::env::var_os("DFD_BUILD_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("dist"));
    run_command(
        Command::new("trunk")
            .args(["build", "--release", "--locked", "--dist"])
            .arg(dist)
            .env("NO_COLOR", "true")
            .current_dir(workspace),
    )
}

fn run_command(command: &mut Command) -> Result<()> {
    eprintln!("Running {command:?}");
    let status = command
        .status()
        .with_context(|| format!("Cannot run {command:?}"))?;
    if !status.success() {
        bail!("Command failed ({status}): {command:?}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_rejects_invalid_fixture_counts_and_suite_names() {
        for args in [
            vec!["xtask", "generate-fixture", "1", "0", "fixture.json"],
            vec!["xtask", "generate-fixture", "2", "-1", "fixture.json"],
            vec!["xtask", "browser-test", "--only", "unknown"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn browser_cli_preserves_explicit_settings_and_defaults() {
        let Task::BrowserTest {
            url,
            output,
            only,
            headed,
        } = Cli::try_parse_from([
            "xtask",
            "browser-test",
            "--url",
            "http://127.0.0.1:8080",
            "--output",
            "reports",
            "--only",
            "startup",
            "--headed",
        ])
        .unwrap()
        .command
        else {
            panic!("Wrong command")
        };
        assert_eq!(url.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(output, PathBuf::from("reports"));
        assert_eq!(only, Suite::Startup);
        assert!(headed);
        let Task::BrowserTest {
            url,
            output,
            only,
            headed,
        } = Cli::try_parse_from(["xtask", "browser-test"])
            .unwrap()
            .command
        else {
            panic!("Wrong command")
        };
        assert!(url.is_none());
        assert_eq!(output, PathBuf::from("/results"));
        assert_eq!(only, Suite::All);
        assert!(!headed);
    }
}
