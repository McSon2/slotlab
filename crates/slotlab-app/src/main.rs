use clap::Parser;
use slotlab_app::{run_server, ServerOptions};

#[derive(Parser)]
#[command(name = "slotlab", about = "Unified dev/QA tool for Stake Engine slots")]
struct Cli {
    /// UI + API port (HTTP).
    #[arg(long, default_value_t = 3600)]
    port: u16,
    /// Emulated RGS port (HTTPS — the games' frontend requires https).
    #[arg(long, default_value_t = 3601)]
    rgs_port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "slotlab=info,tower_http=warn".into()),
        )
        .init();
    let cli = Cli::parse();
    run_server(ServerOptions { port: cli.port, rgs_port: cli.rgs_port }).await
}
