use super::{ROCKER_CONTAINERS_PATH, ROCKER_DB_PATH, ROCKER_IMAGES_PATH, ROCKER_NETNS_PATH};
use std::{
    ffi::CString,
    fs::{self, create_dir_all},
    path::Path,
    str::from_utf8,
};

use anyhow::{anyhow, Context, Result};
use dkregistry::v2::manifest::ManifestSchema2;
use hex::encode;
use nix::{
    fcntl::{open, OFlag},
    libc::mount,
    mount::{umount, MsFlags},
    sched::{clone, setns, CloneFlags},
    sys::{signal::Signal, wait::waitpid},
    unistd::{chdir, chroot, close, execv, sethostname},
    NixPath,
};
use rand::Rng;

use crate::{
    cgroup::create_cgroup,
    db::{used_ip_address_key, veth_ip_address_key, DB},
    image::download_image_if_needed,
    network::{delete_netns, run_in_network_namespace, setup_netns, setup_veths},
};

pub async fn run_container(
    mem: Option<String>,
    cpus: Option<f32>,
    pids: Option<i32>,
    image_name: String,
    registry_username: Option<String>,
    registry_password: Option<String>,
    command: String,
) -> Result<()> {
    let container_id = create_container_id();
    let (image_hash, manifest) =
        download_image_if_needed(image_name, registry_username, registry_password).await?;
    create_container_directories(&container_id)?;
    mount_overlay_fs(&manifest, &container_id, &image_hash)?;
    setup_netns(&container_id).await?;
    setup_veths(&container_id).await?;
    // TODO: configure NAT to connect to internet

    let mnt_path = format!("{}/{}/fs/mnt", ROCKER_CONTAINERS_PATH, &container_id);
    const CONTAINER_STACK_SIZE: usize = 1024 * 1024;
    let mut stack = Box::new([0; CONTAINER_STACK_SIZE]);

    let cb = Box::new(|| {
        let ns_path = format!("{}/{}", ROCKER_NETNS_PATH, &format!("ns-{}", &container_id));
        let mut oflag = OFlag::empty();
        oflag.insert(OFlag::O_RDONLY);
        oflag.insert(OFlag::O_EXCL);

        let fd = open(ns_path.as_str(), oflag, nix::sys::stat::Mode::empty()).unwrap();
        setns(fd, CloneFlags::CLONE_NEWNET).unwrap();
        close(fd).unwrap();

        nix::unistd::sethostname(&container_id);

        chroot(Path::new(&mnt_path));
        chdir("/");

        mount_container_fs();

        execv(
            &CString::new((&command).to_string()).unwrap(),
            &[CString::new((&command).to_string()).unwrap()],
        );

        return 0;
    });

    let clone_flags = CloneFlags::CLONE_NEWNS
        | CloneFlags::CLONE_NEWPID
        | CloneFlags::CLONE_NEWUTS
        | CloneFlags::CLONE_NEWIPC;
    let pid = clone(cb, &mut *stack, clone_flags, Some(Signal::SIGCHLD as i32))
        .with_context(|| "fialed to clone")?;
    create_cgroup(&container_id, pid.as_raw() as u32, mem, cpus, pids);
    waitpid(pid, None)?;
    println!("Container {} done", &container_id);

    umount_container_fs(&mnt_path).unwrap();

    let db = DB.lock().unwrap();
    let res = db.get(veth_ip_address_key(&format!(
        "ns-veth-{}",
        &container_id[0..6]
    )))?;
    if res.is_none() {
        return Err(anyhow!(format!(
            "IP address not found for veth: ns-veth-{}",
            &container_id[0..6]
        )));
    }

    delete_netns(&container_id).await?;
    umount_overlay_fs(&container_id)?;
    fs::remove_dir_all(format!("{}/{}", ROCKER_CONTAINERS_PATH, &container_id))?;
    Ok(())
}

fn create_container_id() -> String {
    let random_bytes = rand::thread_rng().gen::<[u8; 6]>();
    let string = encode(random_bytes);
    println!("new container ID: {}", string);
    string
}

fn create_container_directories(container_id: &String) -> Result<()> {
    let container_path = format!("{}{}{}", ROCKER_CONTAINERS_PATH, "/", container_id);
    let container_directories = [
        format!("{}{}", container_path, "/fs"),
        format!("{}{}", container_path, "/fs/mnt"),
        format!("{}{}", container_path, "/fs/upperdir"),
        format!("{}{}", container_path, "/fs/workdir"),
    ];

    for path in container_directories.iter() {
        fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path))?;
    }
    Ok(())
}

fn mount_overlay_fs(
    manifest: &ManifestSchema2,
    container_id: &String,
    image_hash: &String,
) -> Result<()> {
    let image_base_path = format!("{}{}{}", ROCKER_IMAGES_PATH, "/", image_hash);
    let mut src_layers: Vec<String> = Vec::new();
    for layer in manifest.get_layers() {
        src_layers.push(format!(
            "{}{}{}{}",
            image_base_path,
            "/",
            layer[7..=18].to_string(),
            "/fs",
        ));
    }

    let container_fs_base_path = &format!("{}/{}/fs", ROCKER_CONTAINERS_PATH, container_id);
    let src_layers_str = src_layers.join(":");
    let options: &str = &format!(
        "lowerdir={},upperdir={}/upperdir,workdir={}/workdir",
        src_layers_str, container_fs_base_path, container_fs_base_path
    );

    nix::mount::mount::<Path, Path, [u8], str>(
        None,
        Path::new(&format!("{}/mnt", container_fs_base_path)),
        Some(b"overlay".as_ref()),
        MsFlags::empty(),
        Some(options),
    )?;

    Ok(())
}

fn umount_overlay_fs(container_id: &String) -> Result<()> {
    let mounted_path = format!("{}/{}/fs/mnt", ROCKER_CONTAINERS_PATH, container_id);
    nix::mount::umount(Path::new(&mounted_path))?;
    Ok(())
}

fn prepare_and_execute_container(
    mem: Option<i32>,
    swap: Option<i32>,
    pids: Option<i32>,
    cpus: Option<i32>,
    container_id: &String,
    image_hash: &String,
    command: String,
) -> Result<()> {
    Ok(())
}

fn mount_container_fs() -> Result<()> {
    create_dir_all("/proc");
    nix::mount::mount::<str, Path, [u8], str>(
        Some("proc"),
        Path::new("/proc"),
        Some(b"proc".as_ref()),
        MsFlags::empty(),
        Some(""),
    )
    .unwrap();

    nix::mount::mount::<str, Path, [u8], str>(
        Some("tmpfs"),
        Path::new("/tmp"),
        Some(b"tmpfs".as_ref()),
        MsFlags::empty(),
        Some(""),
    )
    .unwrap();

    nix::mount::mount::<str, Path, [u8], str>(
        Some("tmpfs"),
        Path::new("/dev"),
        Some(b"tmpfs".as_ref()),
        MsFlags::empty(),
        Some(""),
    )
    .unwrap();

    create_dir_all("/dev/pts");
    nix::mount::mount::<str, Path, [u8], str>(
        Some("devpts"),
        Path::new("/dev/pts"),
        Some(b"devpts".as_ref()),
        MsFlags::empty(),
        Some(""),
    )
    .unwrap();

    create_dir_all("/sys");
    nix::mount::mount::<str, Path, [u8], str>(
        Some("sysfs"),
        Path::new("/sys"),
        Some(b"sysfs".as_ref()),
        MsFlags::empty(),
        Some(""),
    )
    .unwrap();

    Ok(())
}

fn umount_container_fs(container_mount_path: &str) -> Result<()> {
    umount(Path::new(&format!("{}/dev/pts", &container_mount_path))).unwrap();
    umount(Path::new(&format!("{}/dev", &container_mount_path))).unwrap();
    umount(Path::new(&format!("{}/sys", &container_mount_path))).unwrap();
    umount(Path::new(&format!("{}/proc", &container_mount_path))).unwrap();
    umount(Path::new(&format!("{}/tmp", &container_mount_path))).unwrap();
    Ok(())
}
