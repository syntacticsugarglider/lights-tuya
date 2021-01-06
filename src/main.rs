use lights_tuya::{AccessToken, HsbColor, Light, TuyaApi};
use serde::{Deserialize, Serialize};
use smol::block_on;
use std::io::{Read, Write};

#[derive(Serialize, Deserialize)]
struct DevicesFile {
    devices: Vec<Light>,
}

fn main() {
    block_on(async move {
        let key_path = std::path::Path::new("access_token");
        let api = if key_path.exists() {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(key_path)
                .unwrap();
            TuyaApi::from_token(AccessToken::read_from(file).unwrap())
        } else {
            let api = TuyaApi::new(
                std::env::var("TUYA_USER").unwrap(),
                std::env::var("TUYA_PASSWORD").unwrap(),
            )
            .await
            .unwrap();
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(key_path)
                .unwrap();
            api.dump_token().write_to(file).unwrap();
            api
        };
        let devices_path = std::path::Path::new("devices.toml");
        let devices = if devices_path.exists() {
            let mut buf = String::new();
            std::fs::File::open(devices_path)
                .unwrap()
                .read_to_string(&mut buf)
                .unwrap();
            let DevicesFile { devices } = toml::from_str(&buf).unwrap();
            devices
        } else {
            let devices = api.scan().await.unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(devices_path)
                .unwrap()
                .write_all(
                    toml::to_string(&DevicesFile {
                        devices: devices.clone(),
                    })
                    .unwrap()
                    .as_bytes(),
                )
                .unwrap();
            devices
        };
        let light_name = std::env::var("TUYA_LIGHT_NAME").unwrap();
        for light in devices {
            if light.name == light_name {
                api.set_color(
                    &light,
                    HsbColor {
                        hue: 0,
                        saturation: 0.,
                        brightness: 0,
                    },
                )
                .await
                .unwrap();
            }
        }
    });
}
