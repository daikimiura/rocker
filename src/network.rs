use std::{
    convert::{TryFrom, TryInto},
    fs::{File, OpenOptions},
    net::{IpAddr, Ipv4Addr},
    os::unix::prelude::{AsRawFd, IntoRawFd},
    path::Path,
    process::exit,
    sync::{Arc, Mutex},
    thread,
};

use futures::{
    channel::mpsc::{UnboundedReceiver, UnboundedSender},
    Future, Sink, Stream, TryStreamExt,
};

use anyhow::{anyhow, Result};
use ipnetwork::IpNetwork;
use nix::{
    self,
    fcntl::{self, open, OFlag},
    sched::{setns, unshare, CloneFlags},
    sys::{
        stat::Mode,
        wait::{waitpid, WaitStatus},
    },
    unistd::{self, close, fork, ForkResult},
};
use rand::Rng;
use rtnetlink::{
    new_connection,
    packet::{
        rtnl::{
            constants::{AF_BRIDGE, RTEXT_FILTER_BRVLAN},
            link::nlas::Nla,
        },
        IFF_LOWER_UP, IFF_UP,
    },
    Handle, NetworkNamespace,
};
use tokio::{runtime::Runtime, task::spawn_blocking};

use crate::{
    db::{used_ip_address_key, veth_ip_address_key, DB},
    fork::fork_fn,
    ROCKER_BRIDGE_ADDRESS, ROCKER_BRIDGE_NAME, ROCKER_DB_PATH, ROCKER_NETNS_PATH,
    ROCKER_NETWORK_ADDRESS,
};

pub async fn is_network_bridge_up() -> Result<bool> {
    let (connection, handle, _) = new_connection().unwrap();

    tokio::spawn(connection);
    let mut links = handle
        .clone()
        .link()
        .get()
        .set_filter_mask(AF_BRIDGE as u8, RTEXT_FILTER_BRVLAN)
        .execute();

    'outer: while let Some(msg) = links.try_next().await? {
        let is_up = msg.header.flags & IFF_UP;
        for nla in msg.nlas.into_iter() {
            if let Nla::IfName(name) = nla {
                if name == ROCKER_BRIDGE_NAME.to_string() && is_up == 1u32 {
                    println!("rocker0 (bridge) is already up");
                    return Ok(true);
                }
                continue 'outer;
            }
        }
    }

    println!("rocker0 (bridge) is not up");
    Ok(false)
}

pub async fn setup_network_bridge() -> Result<()> {
    let (connection, handle, _) = new_connection().unwrap();
    tokio::spawn(connection);

    let mut links = handle
        .link()
        .get()
        .set_name_filter(ROCKER_BRIDGE_NAME.to_string())
        .execute();

    if let Some(link) = links.try_next().await? {
        set_link_up(&handle, &ROCKER_BRIDGE_NAME.to_string()).await?;
        return Ok(());
    };

    handle
        .link()
        .add()
        .bridge(ROCKER_BRIDGE_NAME.to_string())
        .execute()
        .await?;

    let bridge_ip: IpNetwork = ROCKER_BRIDGE_ADDRESS.parse()?;
    let network_addr: IpNetwork = ROCKER_NETWORK_ADDRESS.parse()?;

    let mut links = handle
        .link()
        .get()
        .set_name_filter(ROCKER_BRIDGE_NAME.to_string())
        .execute();

    if let Some(link) = links.try_next().await? {
        handle
            .address()
            .add(link.header.index, bridge_ip.ip(), network_addr.prefix())
            .execute()
            .await?;
        set_link_up(&handle, &ROCKER_BRIDGE_NAME.to_string()).await?;
        return Ok(());
    };

    Err(anyhow!("Failed to create bridge."))
}

pub async fn setup_veths(container_id: &String) -> Result<()> {
    let bridge_side_veth_name = format!("br-veth-{}", container_id[0..6].to_string());
    let container_side_veth_name = format!("ns-veth-{}", container_id[0..6].to_string());

    let (connection, handle, _) = new_connection().unwrap();
    tokio::spawn(connection);
    handle
        .link()
        .add()
        .veth(
            bridge_side_veth_name.clone().into(),
            container_side_veth_name.clone().into(),
        )
        .execute()
        .await?;

    set_link_up(&handle, &bridge_side_veth_name).await?;

    set_link_master(&handle, &bridge_side_veth_name, ROCKER_BRIDGE_NAME).await?;

    add_veth_to_netns(
        &handle,
        &container_side_veth_name,
        &format!("ns-{}", container_id),
    )
    .await?;

    let db = DB.lock().unwrap();
    let ip_addr = Arc::new(create_ip_address(&handle, &db)?);

    run_in_network_namespace(
        &format!("ns-{}", container_id),
        || {
            let c_id = container_id.clone();
            let ip = ip_addr.clone();
            thread::spawn(|| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    let container_side_veth_name_rt = format!("ns-veth-{}", c_id[0..6].to_string());
                    let (connection, handle, _) = new_connection().unwrap();
                    tokio::spawn(connection);

                    add_ip_addr_to_veth(&handle, &container_side_veth_name_rt.to_string(), *ip)
                        .await
                        .unwrap();
                    set_link_up(&handle, &container_side_veth_name_rt.to_string())
                        .await
                        .unwrap();
                    set_default_gateway(&handle, ROCKER_BRIDGE_ADDRESS.parse().unwrap())
                        .await
                        .unwrap();
                    add_ip_address_to_loopback_interface().await.unwrap();
                });
                exit(0);
            })
            .join()
            .expect("Thread paniced");
        },
        true,
    );

    db.insert(
        veth_ip_address_key(&format!("ns-veth-{}", container_id[0..6].to_string())),
        ip_addr.to_string().as_str(),
    )?;

    Ok(())
}

async fn add_veth_to_netns(handle: &Handle, veth_name: &str, netns_name: &str) -> Result<()> {
    let mut links = handle
        .link()
        .get()
        .set_name_filter(veth_name.to_string())
        .execute();
    if let Some(link) = links.try_next().await? {
        let netns_fd = OpenOptions::new()
            .read(true)
            .open(format!("{}/{}", ROCKER_NETNS_PATH, netns_name))?
            .into_raw_fd();
        handle
            .link()
            .set(link.header.index)
            .setns_by_fd(netns_fd)
            .execute()
            .await?;
    } else {
        return Err(anyhow!("Link not found: {}", veth_name));
    }
    Ok(())
}

// set loopback address of current network namespace
async fn add_ip_address_to_loopback_interface() -> Result<()> {
    let (connection, handle, _) = new_connection().unwrap();
    tokio::spawn(connection);
    let mut links = handle
        .link()
        .get()
        .set_name_filter("lo".to_string())
        .execute();

    if let Some(link) = links.try_next().await? {
        let ip_network: IpNetwork = "127.0.0.1/32".parse()?;
        handle
            .address()
            .add(link.header.index, ip_network.ip(), ip_network.prefix())
            .execute()
            .await?;
    } else {
        return Err(anyhow!("Link not found: lo"));
    }

    Ok(())
}

async fn add_ip_addr_to_veth(handle: &Handle, veth_name: &str, ip: IpAddr) -> Result<()> {
    let mut links = handle
        .link()
        .get()
        .set_name_filter(veth_name.to_string())
        .execute();
    if let Some(link) = links.try_next().await? {
        let ip_network: IpNetwork = format!("{}/16", ip.to_string()).parse()?;
        handle
            .address()
            .add(link.header.index, ip_network.ip(), ip_network.prefix())
            .execute()
            .await?;
    } else {
        return Err(anyhow!("Link not found: {}", veth_name));
    }

    Ok(())
}

async fn set_default_gateway(handle: &Handle, default_gateway_ip_addr: Ipv4Addr) -> Result<()> {
    let route = handle.route();
    route
        .add()
        .v4()
        .destination_prefix("0.0.0.0".parse()?, 0)
        .gateway(default_gateway_ip_addr)
        .execute()
        .await?;
    Ok(())
}

pub async fn setup_netns(container_id: &str) -> Result<()> {
    NetworkNamespace::add(format!("ns-{}", container_id)).await?;
    Ok(())
}

pub async fn delete_netns(container_id: &str) -> Result<()> {
    NetworkNamespace::del(format!("ns-{}", container_id)).await?;
    Ok(())
}

async fn set_link_up(handle: &Handle, name: &str) -> Result<()> {
    let mut links = handle
        .link()
        .get()
        .set_name_filter(name.to_string())
        .execute();
    if let Some(link) = links.try_next().await? {
        handle.link().set(link.header.index).up().execute().await?
    } else {
        return Err(anyhow!("Link not found: {}", name));
    }
    Ok(())
}

async fn set_link_master(handle: &Handle, bridge_veth_name: &str, bridge_name: &str) -> Result<()> {
    // Find bridge
    let mut links = handle
        .link()
        .get()
        .set_name_filter(bridge_name.to_string())
        .execute();

    let bridge;
    if let Some(b) = links.try_next().await? {
        bridge = b;
    } else {
        return Err(anyhow!("Link not found: {}", bridge_name));
    }

    // Find bridge-side veth and `set master`
    let mut links = handle
        .link()
        .get()
        .set_name_filter(bridge_veth_name.to_string())
        .execute();
    if let Some(link) = links.try_next().await? {
        handle
            .link()
            .set(link.header.index)
            .master(bridge.header.index)
            .execute()
            .await?
    } else {
        return Err(anyhow!("Link not found: {}", bridge_veth_name));
    }

    Ok(())
}

pub fn run_in_network_namespace(
    netns_name: &str,
    fun: impl FnOnce(),
    blocking: bool,
) -> nix::unistd::Pid {
    fork_fn(
        || {
            let ns_path = format!("{}/{}", ROCKER_NETNS_PATH, netns_name);
            let mut oflag = OFlag::empty();
            oflag.insert(OFlag::O_RDONLY);
            oflag.insert(OFlag::O_EXCL);

            let fd = open(ns_path.as_str(), oflag, Mode::empty()).unwrap();
            setns(fd, CloneFlags::CLONE_NEWNET).unwrap();
            close(fd).unwrap();
            fun();
        },
        blocking,
    )
}

fn create_ip_address(handle: &Handle, db: &sled::Db) -> Result<IpAddr> {
    let mut is_ok = false;
    let mut rand_nums = rand::thread_rng().gen::<[u8; 2]>();
    // let mut new_addr: IpAddr = format!("172.28.{}.{}", rand_nums[0], rand_nums[1]).parse()?;
    let mut new_addr: IpAddr = "172.28.190.151".parse()?;
    while (!is_ok) {
        match db.get(used_ip_address_key(&new_addr.to_string()))? {
            Some(_) => {
                println!("IP address: {} is already in use", new_addr.to_string());
                rand_nums = rand::thread_rng().gen::<[u8; 2]>();
                new_addr = format!("172.28.{}.{}", rand_nums[0], rand_nums[1]).parse()?;
            }
            None => {
                db.insert(used_ip_address_key(&new_addr.to_string()), "1")?;
                db.insert(used_ip_address_key("abc"), "1");
                is_ok = true;
            }
        };
    }

    println!("container's IP address is {}", new_addr.to_string());

    Ok(new_addr)
}
