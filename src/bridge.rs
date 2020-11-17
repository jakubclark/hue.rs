use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::*;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct LightState {
    pub on: bool,
    pub bri: Option<u8>,
    pub hue: Option<u16>,
    pub sat: Option<u8>,
    pub ct: Option<u16>,
    pub xy: Option<(f32, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Light {
    pub name: String,
    pub modelid: String,
    pub swversion: String,
    pub uniqueid: String,
    pub state: LightState,
}

#[derive(Debug, Clone)]
pub struct IdentifiedLight {
    pub id: usize,
    pub light: Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLight {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bri: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hue: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sat: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ct: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xy: Option<(f32, f32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transitiontime: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

impl Default for CommandLight {
    fn default() -> CommandLight {
        CommandLight {
            on: None,
            bri: None,
            hue: None,
            sat: None,
            transitiontime: None,
            ct: None,
            xy: None,
            alert: None,
        }
    }
}

impl CommandLight {
    pub fn on(self) -> CommandLight {
        CommandLight {
            on: Some(true),
            ..self
        }
    }
    pub fn off(self) -> CommandLight {
        CommandLight {
            on: Some(false),
            ..self
        }
    }
    pub fn with_bri(self, b: u8) -> CommandLight {
        CommandLight {
            bri: Some(b),
            ..self
        }
    }
    pub fn with_hue(self, h: u16) -> CommandLight {
        CommandLight {
            hue: Some(h),
            ..self
        }
    }
    pub fn with_sat(self, s: u8) -> CommandLight {
        CommandLight {
            sat: Some(s),
            ..self
        }
    }
    pub fn with_ct(self, c: u16) -> CommandLight {
        CommandLight {
            ct: Some(c),
            ..self
        }
    }
    pub fn with_xy(self, x: f32, y: f32) -> CommandLight {
        CommandLight {
            xy: Some((x, y)),
            ..self
        }
    }
    pub fn alert(self) -> CommandLight {
        CommandLight {
            alert: Some("select".into()),
            ..self
        }
    }
}

#[derive(Debug)]
pub struct Bridge {
    pub ip: std::net::IpAddr,
    pub username: Option<String>,
    client: ureq::Agent,
}

impl Bridge {
    pub fn for_ip(ip: std::net::IpAddr) -> Bridge {
        Bridge {
            ip,
            username: None,
            client: ureq::agent(),
        }
    }

    #[allow(dead_code)]
    pub fn discover() -> Option<Bridge> {
        disco::discover_hue_bridge().ok().map(|ip| Bridge {
            ip,
            username: None,
            client: ureq::agent(),
        })
    }

    pub fn discover_required() -> Bridge {
        Bridge::discover().unwrap_or_else(|| panic!("No bridge found!"))
    }

    pub fn with_user(self, username: String) -> Bridge {
        Bridge {
            username: Some(username),
            ..self
        }
    }

    pub fn register_user(&mut self, devicetype: &str) -> Result<String, HueError> {
        #[derive(Serialize, Deserialize)]
        struct PostApi {
            devicetype: String,
        }
        #[derive(Serialize, Deserialize)]
        struct Success {
            success: Username,
        }
        #[derive(Serialize, Deserialize)]
        struct Username {
            username: String,
        }
        let obtain = PostApi {
            devicetype: devicetype.to_string(),
        };
        let url = format!("http://{}/api", self.ip);
        let resp = self.client.post(&url).send_json(serde_json::to_value(obtain)?);
        if resp.synthetic() {
            return Err(HueError::Ureq(resp.into_synthetic_error().unwrap()));
        }
        let success: Success = resp.into_json_deserialize()?;

        self.username = Some(success.success.username.clone());

        Ok(success.success.username)
    }

    pub fn get_all_lights(&self) -> Result<Vec<IdentifiedLight>, HueError> {
        let url = format!(
            "http://{}/api/{}/lights",
            self.ip,
            self.username.as_ref().ok_or(HueError::NoUsername)?
        );
        let resp = self.client.get(&url).call();
        if resp.synthetic() {
            return Err(HueError::Ureq(resp.into_synthetic_error().unwrap()));
        }
        let resp: HashMap<String, Light> = self.parse(resp.into_json()?)?;

        let mut lights = vec![];
        for (k, v) in resp {
            let id: usize = usize::from_str(&k).or(Err(HueError::ProtocolError {
                msg: "Light id should be a number".to_string(),
            }))?;
            lights.push(IdentifiedLight { id, light: v });
        }
        lights.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(lights)
    }

    pub fn set_light_state(&self, light: usize, command: &CommandLight) -> Result<Value, HueError> {
        let url = format!(
            "http://{}/api/{}/lights/{}/state",
            self.ip,
            self.username.as_ref().ok_or(HueError::NoUsername)?,
            light
        );
        let resp = self.client.put(&url).build().send_json(serde_json::to_value(command)?);
        if resp.synthetic() {
            return Err(HueError::Ureq(resp.into_synthetic_error().unwrap()));
        }
        self.parse(resp.into_json()?)
    }

    fn parse<T: ::serde::de::DeserializeOwned>(&self, value: Value) -> Result<T, HueError> {
        use serde_json::*;
        if !value.is_array() {
            return Ok(from_value(value)?);
        }
        let mut objects: Vec<Value> = from_value(value)?;
        if objects.len() == 0 {
            Err(HueError::ProtocolError {
                msg: "expected non-empty array".to_string(),
            })?
        }
        let value = objects.remove(0);
        {
            let object = value.as_object().ok_or(HueError::ProtocolError {
                msg: "expected first item to be an object".to_string(),
            })?;
            if let Some(e) = object.get("error").and_then(|o| o.as_object()) {
                let code: u64 = e.get("type").and_then(|s| s.as_u64()).unwrap_or(0);
                let desc = e
                    .get("description")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                Err(HueError::BridgeError {
                    code: code as usize,
                    msg: desc,
                })?
            }
        }
        Ok(from_value(value)?)
    }
}
