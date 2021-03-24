use anyhow::{anyhow, Context, Result};
use clap::Clap;
use container::{print_running_containers, run_container};
use image::print_available_images;
use network::{is_network_bridge_up, setup_network_bridge};
use std::{
    fs::{self, OpenOptions},
    path::Path,
};

const ROCKER_TMP_PATH: &str = "/var/lib/rocker/tmp";
const ROCKER_IMAGES_PATH: &str = "/var/lib/rocker/images";
const ROCKER_DB_PATH: &str = "/var/lib/rocker/db";
const ROCKER_CONTAINERS_PATH: &str = "/var/run/rocker/containers";
const ROCKER_NETNS_PATH: &str = "/run/netns";
const ROCKER_BRIDGE_NAME: &str = "rocker0";
const ROCKER_NETWORK_ADDRESS: &str = "172.28.0.0/16";
const ROCKER_BRIDGE_ADDRESS: &str = "172.28.0.1";

mod cgroup;
mod container;
mod db;
mod dbus_systemd;
mod fork;
mod image;
mod network;

#[derive(Clap)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    Run(Run),
    Ps,
    Exec,
    Images,
    Rmi,
}

#[derive(Clap)]
struct Run {
    #[clap(short, long)]
    mem: Option<String>,
    #[clap(long)]
    cpus: Option<f32>,
    #[clap(long)]
    pids_limit: Option<i32>,
    #[clap(short, long)]
    username: Option<String>,
    #[clap(short, long)]
    password: Option<String>,
    image_name: String,
    command: String,
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();

    if !nix::unistd::getuid().is_root() {
        return Err(anyhow!("You need root privileges to run this program."));
    };

    init_dirs()?;

    match opts.subcmd {
        SubCommand::Run(r) => {
            let mut rt = tokio::runtime::Runtime::new()?;

            let task = async {
                if let is_up = is_network_bridge_up().await? {
                    if !is_up {
                        setup_network_bridge().await?
                    }
                };
                run_container(
                    r.mem,
                    r.cpus,
                    r.pids_limit,
                    r.image_name,
                    r.username,
                    r.password,
                    r.command,
                )
                .await
            };
            rt.block_on(task)?
        }
        SubCommand::Ps => print_running_containers()?,
        SubCommand::Images => print_available_images()?,
        _ => (),
    };

    Ok(())
}

fn init_dirs() -> Result<()> {
    let dirs = [ROCKER_TMP_PATH, ROCKER_IMAGES_PATH, ROCKER_CONTAINERS_PATH];

    for path in dirs.iter() {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path))?;
    }

    Ok(())
}
