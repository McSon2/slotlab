use clap::Parser;
use slotlab_app::{run_hub, run_server, HubOptions, ServerOptions};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "slotlab", about = "Unified dev/QA tool for Stake Engine slots")]
struct Cli {
    /// UI + API port (HTTP).
    #[arg(long, default_value_t = 3600)]
    port: u16,
    /// Emulated RGS port (HTTPS — the games' frontend requires https).
    #[arg(long, default_value_t = 3601)]
    rgs_port: u16,

    /// Headless mode: publicly serve every game published under
    /// --data-dir, routed by the request's Host.
    #[arg(long)]
    hub: bool,
    /// Directory holding one sub-directory per published game.
    #[arg(long, env = "SLOTLAB_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Address to bind in hub mode.
    #[arg(long, env = "SLOTLAB_BIND", default_value = "0.0.0.0:3610")]
    bind: String,
    /// Hostname of the admin dashboard (behind Cloudflare Access).
    #[arg(long, env = "SLOTLAB_ADMIN_HOST")]
    admin_host: Option<String>,
    /// Token required by the control API /api/hub/*.
    #[arg(long, env = "SLOTLAB_ADMIN_TOKEN")]
    admin_token: Option<String>,
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
    if cli.hub {
        return run_hub(HubOptions {
            data_dir: cli
                .data_dir
                .ok_or_else(|| anyhow::anyhow!("--hub requires --data-dir"))?,
            bind: cli.bind.parse()?,
            admin_host: cli.admin_host,
            admin_token: cli.admin_token,
        })
        .await;
    }
    run_server(ServerOptions {
        port: cli.port,
        rgs_port: cli.rgs_port,
    })
    .await
}
