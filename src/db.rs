use once_cell::sync::Lazy;
use std::{net::IpAddr, sync::Mutex};

use crate::ROCKER_DB_PATH;

const USED_IP_ADDRESSES_KEY_PREFIX: &str = "used_ip_addresses";
const VETH_IP_ADDRESSES_KEY_PREFIX: &str = "veth_ip_addresses";
const DOWNLOADED_IMAGES_KEY_PREFIX: &str = "downloaded_images";
const CONTAINER_COMMANDS_KEY_PREFIX: &str = "container_commands";
const CONTAINER_IMAGE_HASHES_KEY_PREFIX: &str = "container_image_hashes";

pub fn used_ip_addresses_key(key: &str) -> String {
    format!("{}/{}", USED_IP_ADDRESSES_KEY_PREFIX, key)
}

pub fn veth_ip_addresses_key(key: &str) -> String {
    format!("{}/{}", VETH_IP_ADDRESSES_KEY_PREFIX, key)
}

// image_hash => image_name (name:tag)
pub fn downloaded_images_key(key: &str) -> String {
    format!("{}/{}", DOWNLOADED_IMAGES_KEY_PREFIX, key)
}

// container_id => command
pub fn container_commands_key(key: &str) -> String {
    format!("{}/{}", CONTAINER_COMMANDS_KEY_PREFIX, key)
}

// container_id => image_hash
pub fn container_image_hashes_key(key: &str) -> String {
    format!("{}/{}", CONTAINER_IMAGE_HASHES_KEY_PREFIX, key)
}
