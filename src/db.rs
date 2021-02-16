use once_cell::sync::Lazy;
use std::{net::IpAddr, sync::Mutex};

use crate::ROCKER_DB_PATH;

const USED_IP_ADDRESS_KEY_PREFIX: &str = "used_ip_addresses";
const VETH_IP_ADDRESS_KEY_PREFIX: &str = "veth_ip_addresses";
const IMAGE_HASH_KEY_PREFIX: &str = "image_hashes";

pub static DB: Lazy<Mutex<sled::Db>> =
    Lazy::new(|| Mutex::new(sled::open(ROCKER_DB_PATH).unwrap()));

pub fn used_ip_address_key(key: &str) -> String {
    format!("{}-{}", USED_IP_ADDRESS_KEY_PREFIX, key)
}

pub fn veth_ip_address_key(key: &str) -> String {
    format!("{}-{}", VETH_IP_ADDRESS_KEY_PREFIX, key)
}

pub fn image_hash_key(key: &str) -> String {
    format!("{}-{}", IMAGE_HASH_KEY_PREFIX, key)
}
