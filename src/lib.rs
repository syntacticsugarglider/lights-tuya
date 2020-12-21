use std::io::Write;
use std::{fmt::Display, io::Read};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use encoding::all::ISO_8859_1;
use encoding::{DecoderTrap, Encoding};

struct TuyaBizType;

impl Serialize for TuyaBizType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        "smart_life".serialize(serializer)
    }
}

struct TuyaFrom;

impl Serialize for TuyaFrom {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        "tuya".serialize(serializer)
    }
}

enum TuyaCountryCode {
    Us,
}

impl Serialize for TuyaCountryCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        1.serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    user_name: String,
    password: String,
    country_code: TuyaCountryCode,
    biz_type: TuyaBizType,
    from: TuyaFrom,
}

impl Credentials {
    fn new(user_name: String, password: String) -> Credentials {
        Credentials {
            user_name,
            password,
            country_code: TuyaCountryCode::Us,
            biz_type: TuyaBizType,
            from: TuyaFrom,
        }
    }
}

#[derive(Debug)]
enum TuyaEndpoint {
    Login,
    Skill,
}

impl Display for TuyaEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuyaEndpoint::Login => write!(f, "auth.do"),
            TuyaEndpoint::Skill => write!(f, "skill"),
        }
    }
}

fn endpoint_uri(endpoint: TuyaEndpoint) -> String {
    format!("https://px1.tuyaus.com/homeassistant/{}", endpoint)
}

pub struct TuyaApi {
    tokens: TuyaApiTokens,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(surf::Error),
    #[error("decoding API response failed: {0}")]
    Encoding(String),
    #[error("deserializing API response failed: {0}")]
    Deserializing(#[source] serde_json::Error),
    #[error("API error: {0}")]
    Api(String),
}

impl From<surf::Error> for Error {
    fn from(error: surf::Error) -> Self {
        Error::Http(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Error::Deserializing(error)
    }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum LoginResponse {
    Success(TuyaApiTokens),
    #[serde(rename_all = "camelCase")]
    Error {
        error_msg: String,
        response_status: String,
    },
}

impl LoginResponse {
    fn to_result(self) -> Result<TuyaApiTokens, Error> {
        match self {
            LoginResponse::Success(token) => Ok(token),
            LoginResponse::Error { error_msg, .. } => Err(Error::Api(error_msg)),
        }
    }
}

#[derive(Deserialize, Debug)]
struct TuyaApiTokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct LightResponse {
    brightness: String,
    color_mode: String,
    online: bool,
    state: String,
    color_temp: i32,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "dev_type")]
enum ScanDevice {
    #[serde(rename = "light")]
    Light {
        data: LightResponse,
        name: String,
        id: String,
    },
    Unknown,
}

#[derive(Deserialize, Debug)]
struct ScanDevices {
    devices: Vec<ScanDevice>,
}

#[derive(Deserialize, Debug)]
struct ScanResponse {
    payload: ScanDevices,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Light {
    pub name: String,
    device_id: DeviceId,
}

impl Light {
    pub fn id(&self) -> &str {
        &self.device_id.0
    }
}

pub struct AccessToken(String);

impl AccessToken {
    pub fn write_to<T: Write>(&self, mut writer: T) -> Result<(), std::io::Error> {
        writer.write_all(self.0.as_bytes())
    }
    pub fn read_from<T: Read>(mut reader: T) -> Result<Self, std::io::Error> {
        let mut buf = String::new();
        reader.read_to_string(&mut buf)?;
        Ok(AccessToken(buf))
    }
}

#[derive(Serialize, Deserialize)]
struct RefreshToken(String);

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(transparent)]
struct DeviceId(String);

enum TuyaCommand {
    Discovery,
    TurnOnOff {
        device_id: DeviceId,
        state: State,
    },
    SetBrightness {
        device_id: DeviceId,
        brightness: u8,
    },
    QueryDevice {
        device_id: DeviceId,
    },
    SetColor {
        device_id: DeviceId,
        color: HsbColor,
    },
    SetColorTemperature {
        device_id: DeviceId,
        temperature: u32,
    },
}

struct TuyaRequest {
    command: TuyaCommand,
    access_token: AccessToken,
}

#[derive(Serialize)]
struct RawRequest {
    header: RawHeader,
    payload: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RawHeader {
    payload_version: i32,
    namespace: String,
    name: String,
}

impl Serialize for TuyaRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (namespace, name, dev_id, value) = match &self.command {
            TuyaCommand::Discovery => ("discovery", "Discovery", None, None),
            TuyaCommand::TurnOnOff { device_id, state } => (
                "control",
                "turnOnOff",
                Some(device_id.clone()),
                Some((
                    "value".to_owned(),
                    match state {
                        State::On => "1",
                        State::Off => "0",
                    }
                    .to_owned()
                    .into(),
                )),
            ),
            TuyaCommand::SetBrightness {
                device_id,
                brightness,
            } => (
                "control",
                "brightnessSet",
                Some(device_id.clone()),
                Some(("value".to_owned(), (*brightness).into())),
            ),
            TuyaCommand::QueryDevice { device_id } => {
                ("query", "QueryDevice", Some(device_id.clone()), None)
            }
            TuyaCommand::SetColor { device_id, color } => (
                "control",
                "colorSet",
                Some(device_id.clone()),
                Some((
                    "color".to_owned(),
                    serde_json::Value::Object(
                        vec![
                            ("hue".into(), color.hue.into()),
                            ("saturation".into(), color.saturation.into()),
                            ("brightness".into(), color.brightness.into()),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                )),
            ),
            TuyaCommand::SetColorTemperature {
                device_id,
                temperature,
            } => (
                "control",
                "colorTemperatureSet",
                Some(device_id.clone()),
                Some(("value".to_owned(), (*temperature).into())),
            ),
        };
        RawRequest {
            header: RawHeader {
                payload_version: 1,
                namespace: namespace.into(),
                name: name.into(),
            },
            payload: serde_json::Value::Object(
                vec![("accessToken".to_owned(), self.access_token.0.clone().into())]
                    .into_iter()
                    .chain(dev_id.map(|id| ("devId".to_string(), serde_json::Value::String(id.0))))
                    .chain(value)
                    .collect(),
            ),
        }
        .serialize(serializer)
    }
}

impl TuyaApiTokens {
    fn access_token(&self) -> AccessToken {
        AccessToken(self.access_token.clone())
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "code")]
enum ResponseType {
    #[serde(rename = "SUCCESS")]
    Success,
    TargetOffline,
}

#[derive(Deserialize, Debug)]
struct SetStateResponse {
    header: ResponseType,
}

impl TuyaApi {
    pub async fn new<T: AsRef<str>, U: AsRef<str>>(user: T, pass: U) -> Result<Self, Error> {
        let mut data = surf::post(endpoint_uri(TuyaEndpoint::Login))
            .body(surf::Body::from_form(&Credentials::new(
                user.as_ref().into(),
                pass.as_ref().into(),
            ))?)
            .content_type("application/x-www-form-urlencoded")
            .send()
            .await?;
        let data = ISO_8859_1
            .decode(&data.body_bytes().await?, DecoderTrap::Strict)
            .map_err(|e| Error::Encoding(e.into_owned()))?;
        let tokens = serde_json::from_str::<LoginResponse>(&data)?.to_result()?;
        Ok(TuyaApi { tokens })
    }
    pub fn dump_token(&self) -> AccessToken {
        self.tokens.access_token()
    }
    pub fn from_token(token: AccessToken) -> Self {
        TuyaApi {
            tokens: TuyaApiTokens {
                access_token: token.0,
                refresh_token: None,
                token_type: None,
                expires_in: None,
            },
        }
    }
    pub async fn scan(&self) -> Result<Vec<Light>, Error> {
        let request = TuyaRequest {
            command: TuyaCommand::Discovery,
            access_token: self.tokens.access_token(),
        };
        let data: ScanResponse = surf::post(endpoint_uri(TuyaEndpoint::Skill))
            .content_type("application/json")
            .body(surf::Body::from_json(&request)?)
            .recv_json()
            .await?;
        Ok(data
            .payload
            .devices
            .into_iter()
            .filter_map(|item| match item {
                ScanDevice::Light {
                    name,
                    id: device_id,
                    ..
                } => Some(Light {
                    name,
                    device_id: DeviceId(device_id),
                }),
                ScanDevice::Unknown => None,
            })
            .collect())
    }
    async fn send_state_command(&self, command: TuyaCommand) -> Result<(), Error> {
        let request = TuyaRequest {
            command,
            access_token: self.tokens.access_token(),
        };
        let data: SetStateResponse = surf::post(endpoint_uri(TuyaEndpoint::Skill))
            .content_type("application/json")
            .body(surf::Body::from_json(&request)?)
            .recv_json()
            .await?;
        if let ResponseType::Success = data.header {
            Ok(())
        } else {
            Err(Error::Api(format!("{:?}", data.header)))
        }
    }
    pub async fn set_state(&self, light: &Light, state: State) -> Result<(), Error> {
        self.send_state_command(TuyaCommand::TurnOnOff {
            device_id: light.device_id.clone(),
            state,
        })
        .await
    }
    pub async fn set_brightness(&self, light: &Light, brightness: u8) -> Result<(), Error> {
        self.send_state_command(TuyaCommand::SetBrightness {
            device_id: light.device_id.clone(),
            brightness: ((brightness as f64 / 255.) * 100.) as u8,
        })
        .await
    }
    pub async fn set_color(&self, light: &Light, color: HsbColor) -> Result<(), Error> {
        self.send_state_command(TuyaCommand::SetColor {
            device_id: light.device_id.clone(),
            color,
        })
        .await
    }
    pub async fn set_color_temperature(
        &self,
        light: &Light,
        temperature: u32,
    ) -> Result<(), Error> {
        self.send_state_command(TuyaCommand::SetColorTemperature {
            device_id: light.device_id.clone(),
            temperature: (1000. + (((temperature.min(6500) - 2700) as f64) / 3800.) * 9000.) as u32,
        })
        .await
    }
    async fn query(&self, light: &Light) -> Result<(), Error> {
        let request = TuyaRequest {
            command: TuyaCommand::QueryDevice {
                device_id: light.device_id.clone(),
            },
            access_token: self.tokens.access_token(),
        };
        let data = surf::post(endpoint_uri(TuyaEndpoint::Skill))
            .content_type("application/json")
            .body(surf::Body::from_json(&request)?)
            .recv_string()
            .await?;
        println!("{}", data);
        Ok(())
    }
}

pub enum State {
    On,
    Off,
}

pub struct HsbColor {
    pub brightness: u16,
    pub saturation: u16,
    pub hue: u16,
}
