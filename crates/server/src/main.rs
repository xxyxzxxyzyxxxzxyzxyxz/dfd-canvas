use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(version, about = "Serve the compiled DFD Canvas application")]
struct Args {
    /// Address and port to listen on.
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,

    /// Directory containing the compiled frontend and index.html.
    #[arg(long, default_value = "dist")]
    dir: PathBuf,

    /// Browser URL to display when the public address differs from --listen.
    #[arg(long)]
    public_url: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dfd-server: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> io::Result<()> {
    let router = dfd_server::router(&args.dir)?;
    let listener = TcpListener::bind(args.listen).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot listen on {}: {error}", args.listen),
        )
    })?;
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    eprintln!(
        "DFD Canvas: {} (serving {})",
        browser_url(listener.local_addr()?, args.public_url.as_deref()),
        args.dir.display()
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let interrupt = async {
                if let Err(error) = tokio::signal::ctrl_c().await {
                    eprintln!("dfd-server: cannot watch for Ctrl+C: {error}");
                }
            };
            #[cfg(unix)]
            tokio::select! {
                () = interrupt => {},
                _ = terminate.recv() => {},
            }
            #[cfg(not(unix))]
            interrupt.await;
        })
        .await
}

fn browser_url(mut address: SocketAddr, public_url: Option<&str>) -> String {
    if let Some(url) = public_url {
        return url.to_owned();
    }
    if address.ip().is_unspecified() {
        address.set_ip(match address.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        });
    }
    format!("http://{address}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_listeners_display_loopback_addresses() {
        assert_eq!(
            browser_url("0.0.0.0:8080".parse().unwrap(), None),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            browser_url("[::]:8080".parse().unwrap(), None),
            "http://[::1]:8080"
        );
        assert_eq!(
            browser_url("192.0.2.1:8081".parse().unwrap(), None),
            "http://192.0.2.1:8081"
        );
    }

    #[test]
    fn public_url_can_display_a_mapped_port_without_changing_the_listener() {
        let args = Args::parse_from(["dfd-server", "--public-url", "http://127.0.0.1:8081"]);
        assert_eq!(args.listen, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(
            browser_url(args.listen, args.public_url.as_deref()),
            "http://127.0.0.1:8081"
        );
    }
}
