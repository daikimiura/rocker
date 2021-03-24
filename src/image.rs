use super::{db::image_hash_key, ROCKER_DB_PATH, ROCKER_IMAGES_PATH, ROCKER_TMP_PATH};
use std::{fs, io::Write, net::ToSocketAddrs};

use anyhow::{anyhow, Result};
use dkregistry::v2::{
    manifest::{Manifest, ManifestSchema2},
    Client,
};
use flate2::read::GzDecoder;
use futures::{future::join_all, join};
use tar::Archive;

struct Image {
    image_hash: String,
    name: String,
    tag: String,
}

pub async fn download_image_if_needed(
    image_name: &str,
    username: Option<String>,
    password: Option<String>,
) -> Result<(String, ManifestSchema2)> {
    let (image_name, tag) = parse_image_name(&image_name)?;
    println!("Downloading metadata for {}:{}", image_name, tag);

    let registry = "index.docker.io";
    let client = Client::configure()
        .registry(registry)
        .insecure_registry(false)
        .username(username)
        .password(password)
        .build()?;
    let login_scope = format!("repository:{}:pull", image_name);
    let dclient = client.authenticate(&[&login_scope]).await?;
    let manifest = dclient.get_manifest(&image_name, &tag).await;

    let s2_manifest: ManifestSchema2 = match manifest {
        Ok(Manifest::S2(m)) => Ok(m),
        Err(e) => Err(anyhow!(e)),
        _ => Err(anyhow!("Image manifest type invalid")),
    }?;

    let image_hash = s2_manifest.manifest_spec.config().digest[7..=18].to_string();

    let db = sled::open(ROCKER_DB_PATH).unwrap();
    if (!is_image_already_downloaded(&db, &image_hash)?) {
        let image_layer_digests = s2_manifest.get_layers();
        println!("Downloading image {}:{}...", image_name, tag);

        download_image(&dclient, &image_name, &image_hash, &image_layer_digests).await?;
        db.insert(
            image_hash_key(&image_hash),
            format!("{}:{}", &image_name, &tag).as_str(),
        )?;
    } else {
        println!("Image already exists");
    }

    Ok((image_hash, s2_manifest))
}

fn is_image_already_downloaded(image_hash_table: &sled::Tree, image_hash: &str) -> Result<bool> {
    match image_hash_table.get(image_hash_key(image_hash))? {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

fn parse_image_name(image_name: &str) -> Result<(String, String)> {
    let s = image_name.split(':').collect::<Vec<&str>>();
    match s.len() {
        2 => {
            if s[0].contains('/') {
                Ok((s[0].to_string(), s[1].to_string()))
            } else {
                Ok(("library/".to_string() + s[0], s[1].to_string()))
            }
        }
        1 => {
            if s[0].contains('/') {
                Ok((s[0].to_string(), "latest".to_string()))
            } else {
                Ok(("library/".to_string() + s[0], "latest".to_string()))
            }
        }
        _ => Err(anyhow!("too many colons")),
    }
}

async fn download_image(
    client: &Client,
    image_name: &str,
    image_hash: &str,
    image_layer_digests: &Vec<String>,
) -> Result<()> {
    download_layers_blob(client, image_name, image_hash, image_layer_digests).await?;
    extract_layers(image_hash, image_layer_digests)?;
    // store_image_metadata();
    delete_temp_image_files(image_hash)?;
    Ok(())
}

async fn download_layers_blob(
    client: &Client,
    image_name: &str,
    image_hash: &str,
    image_layer_digests: &Vec<String>,
) -> Result<()> {
    let image_layers_tar_path = format!("{}{}{}", ROCKER_TMP_PATH, "/", image_hash);
    fs::create_dir_all(&image_layers_tar_path);
    let mut pull_tasks = Vec::new();
    for layer_digest in image_layer_digests {
        println!("Pulling layer: {}", &layer_digest[7..=18]);
        let c = client.clone();
        let tar_path = image_layers_tar_path.clone();
        pull_tasks.push(async move {
            let blob = c.get_blob(image_name, &layer_digest).await;
            let mut file = fs::File::create(format!(
                "{}{}{}{}",
                &tar_path,
                "/",
                &layer_digest[7..=18],
                ".tar.gz"
            ));
            file.unwrap().write(&blob.unwrap()).unwrap();
            println!("Pull complete layer: {}", &layer_digest[7..=18]);
        });
    }

    join_all(pull_tasks).await;

    Ok(())
}

fn extract_layers(image_hash: &str, image_layer_digests: &Vec<String>) -> Result<()> {
    println!("Extract layers...");
    let image_layers_tar_path = format!("{}{}{}", ROCKER_TMP_PATH, "/", image_hash);
    let image_layers_dst_path = format!("{}{}{}", ROCKER_IMAGES_PATH, "/", image_hash,);

    for layer_digest in image_layer_digests {
        // https://rust-lang-nursery.github.io/rust-cookbook/compression/tar.html
        let tar_gz = fs::File::open(format!(
            "{}{}{}{}",
            &image_layers_tar_path,
            "/",
            &layer_digest[7..=18],
            ".tar.gz"
        ))?;

        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);
        let dst_path = format!(
            "{}{}{}{}",
            &image_layers_dst_path,
            "/",
            &layer_digest[7..=18],
            "/fs"
        );
        archive.unpack(dst_path);
    }
    Ok(())
}

fn delete_temp_image_files(image_hash: &str) -> Result<()> {
    let path = format!("{}{}{}", ROCKER_TMP_PATH, "/", image_hash);
    fs::remove_dir_all(path)?;
    Ok(())
}

pub fn print_available_images() -> Result<()> {
    println!("REPOSITORY\tTAG\tIMAGE ID");

    for image in fetch_available_images()? {
        println!("{}\t{}\t{}", image.name, image.tag, image.image_hash)
    }
    Ok(())
}

fn fetch_available_images() -> Result<Vec<Image>> {
    let mut images = Vec::new();

    let db = sled::open(ROCKER_DB_PATH)?;
    for entry in fs::read_dir(ROCKER_IMAGES_PATH)? {
        let path = entry?.path();
        let image_hash = path.file_name().unwrap().to_string_lossy().to_string();

        let image_name_and_tag_res = db.get(image_hash_key(&image_hash)).unwrap().unwrap();
        let image_name_and_tag = String::from_utf8(image_name_and_tag_res.to_vec()).unwrap();
        let image_name_and_tag: Vec<&str> = image_name_and_tag.split(":").collect();

        images.push(Image {
            image_hash: image_hash,
            name: image_name_and_tag[0].to_string(),
            tag: image_name_and_tag[1].to_string(),
        })
    }

    Ok(images)
}
