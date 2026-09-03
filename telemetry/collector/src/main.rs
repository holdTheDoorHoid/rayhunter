//! `rayhunter-collector`: the community side of contributed recordings.
//!
//! One binary, three roles, so an operator has one thing to deploy:
//!
//! - `serve` receives submissions from units. It holds the ingest private
//!   key and nothing else secret; raw captures arrive encrypted to a key it
//!   does not have.
//! - `list`, `show`, `review`, `decrypt` are the triage tools. Nothing is
//!   published until a person has marked it `verified`.
//! - `publish` writes a static site: a master list, a map, a page per
//!   submission, and machine-readable feeds.
//!
//! See `telemetry/DESIGN.md` for the reasoning, and `README.md` beside this
//! crate for deployment.

mod ingest;
mod keys;
mod publish;
mod server;
mod store;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rayhunter-collector", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make the service's two key pairs. Move `archive.key` off this
    /// machine before serving: the server must never hold it.
    Keygen {
        /// Directory to write the four key files into.
        #[arg(long)]
        out: PathBuf,
    },
    /// Receive submissions.
    Serve {
        /// Where submissions are kept.
        #[arg(long)]
        data: PathBuf,
        /// Directory holding `ingest.key`, `ingest.pub` and, to accept full
        /// submissions, `archive.pub`. Defaults to `<data>/keys`.
        #[arg(long)]
        keys: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1:8090")]
        bind: SocketAddr,
        /// The service's name, as units show it to their owners.
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// How to reach whoever runs this.
        #[arg(long)]
        contact: Option<String>,
        /// Where the published site is.
        #[arg(long)]
        site_url: Option<String>,
        /// Accept full submissions (needs `archive.pub`).
        #[arg(long)]
        accept_full: bool,
        #[arg(long, default_value_t = 64)]
        max_summary_mb: u64,
        #[arg(long, default_value_t = 512)]
        max_capture_mb: u64,
        /// Take the client address from the last `X-Forwarded-For` entry,
        /// for a reverse proxy in front. Never set this when clients can
        /// reach the port directly.
        #[arg(long)]
        behind_proxy: bool,
        /// Stop accepting new submissions when the data directory holds
        /// more than this.
        #[arg(long, default_value_t = 50)]
        max_disk_gb: u64,
    },
    /// List submissions.
    List {
        #[arg(long)]
        data: PathBuf,
        /// Only this status: pending, received, verified, rejected, withdrawn.
        #[arg(long)]
        status: Option<String>,
    },
    /// Show one submission in full.
    Show {
        #[arg(long)]
        data: PathBuf,
        id: String,
    },
    /// Record a reviewer's judgement.
    Review {
        #[arg(long)]
        data: PathBuf,
        id: String,
        /// `verified` publishes it; `rejected` keeps it out.
        #[arg(long)]
        status: String,
        /// Tags such as interesting, vulnerable, confirmed, false-positive.
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        reviewer: Option<String>,
    },
    /// Decrypt a full submission's capture with the archive key. Meant for
    /// a machine that is not the server.
    Decrypt {
        #[arg(long)]
        data: PathBuf,
        id: String,
        /// The `archive.key` file.
        #[arg(long)]
        archive_key: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write the public site from every verified submission.
    Publish {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "Community Rayhunter Dataset")]
        title: String,
        /// Where the site will be served from, for absolute links in feeds.
        #[arg(long)]
        base_url: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Keygen { out } => keys::keygen(&out).await,
        Command::Serve {
            data,
            keys,
            bind,
            name,
            description,
            contact,
            site_url,
            accept_full,
            max_summary_mb,
            max_capture_mb,
            behind_proxy,
            max_disk_gb,
        } => {
            let keys_dir = keys.unwrap_or_else(|| data.join("keys"));
            let loaded = keys::load_for_serving(&keys_dir).await?;
            if accept_full && loaded.archive_public.is_none() {
                return Err(anyhow!(
                    "--accept-full needs archive.pub in {}",
                    keys_dir.display()
                ));
            }
            let ctx = server::ServerCtx::new(server::ServerOptions {
                data_dir: data,
                name,
                description,
                contact,
                site_url,
                ingest_private: loaded.ingest_private,
                ingest_public: loaded.ingest_public,
                archive_public: if accept_full {
                    loaded.archive_public
                } else {
                    None
                },
                max_summary_bytes: max_summary_mb * 1024 * 1024,
                max_capture_bytes: max_capture_mb * 1024 * 1024,
                behind_proxy,
                max_disk_bytes: max_disk_gb * 1024 * 1024 * 1024,
            })
            .await?;
            server::serve(ctx, bind).await
        }
        Command::List { data, status } => {
            let filter = status
                .map(|s| store::Status::parse(&s).ok_or_else(|| anyhow!("unknown status {s}")))
                .transpose()?;
            for record in store::list(&data).await? {
                if filter.is_some_and(|f| f != record.status) {
                    continue;
                }
                println!("{}", record.one_line());
            }
            Ok(())
        }
        Command::Show { data, id } => {
            let record = store::load(&data, &id)
                .await?
                .ok_or_else(|| anyhow!("no submission {id}"))?;
            println!("{}", serde_json::to_string_pretty(&record)?);
            if let Some(summary) = store::load_summary(&data, &id).await? {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            }
            Ok(())
        }
        Command::Review {
            data,
            id,
            status,
            tags,
            note,
            reviewer,
        } => {
            let status = match status.as_str() {
                "verified" => store::Status::Verified,
                "rejected" => store::Status::Rejected,
                other => return Err(anyhow!("status must be verified or rejected, not {other}")),
            };
            store::review(&data, &id, status, tags, note, reviewer).await?;
            println!("recorded");
            Ok(())
        }
        Command::Decrypt {
            data,
            id,
            archive_key,
            out,
        } => {
            let key = keys::load_private(&archive_key)
                .await
                .context("reading the archive key")?;
            ingest::decrypt_capture(&data, &id, &key, &out).await?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Command::Publish {
            data,
            out,
            title,
            base_url,
        } => {
            let count = publish::publish(&data, &out, &title, base_url.as_deref()).await?;
            println!("published {count} submissions to {}", out.display());
            Ok(())
        }
    }
}
