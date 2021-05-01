use crate::channels;
use crate::util;
use async_channel::{Receiver, SendError, Sender};
use async_process::{Child, Command, Stdio};
use async_signals::Signals;
use defer::defer;
use futures_lite::future::or;
use futures_lite::io::{AsyncBufReadExt, BufReader, ReadHalf, WriteHalf};
use futures_lite::prelude::*;
use log::debug;
use log::warn;
use serde::{self, Deserialize, Serialize};
use serde_json::json;
use serde_tuple::Serialize_tuple;
use smol::net::unix::UnixStream;
use smol::stream::StreamExt;
use smol::Timer;
use std::path::PathBuf;
use std::time::Duration;

quick_error! {
    #[derive(Debug)]
    pub enum MpvError {
        IOError(err: std::io::Error) {
            from()
        }
        ExitError(err: ExitError) {
            from()
        }
        MpvStartupError(err: MpvStartupError) {
            from()
        }
        SendError(err: SendError<Event>) {
            from()
        }
        JSONError(err: serde_json::Error) {
            from()
        }
        EventParsingError(err: EventParsingError) {
            from()
        }

    }
}

#[derive(Debug)]
pub struct EventParsingError {
    reason: String,
}

#[derive(Debug)]
pub struct ExitError {
    status_code: i32,
    reason: String,
}

#[derive(Debug)]
pub struct MpvStartupError {
    reason: String,
}

#[derive(Debug, Serialize_tuple)]
pub struct Request {
    command: RequestType,
    v1: serde_json::Value,
    v2: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub enum RequestType {
    #[serde(rename = "keybind")]
    Keybind,
    #[serde(rename = "observe_property")]
    ObserveProperty,
    #[serde(rename = "set_property")]
    SetProperty,
    #[serde(rename = "osd-msg")]
    OSDMsg,
}

#[derive(Debug, Serialize)]
pub enum Property {
    #[serde(rename = "time-pos")]
    TimePos,
    #[serde(rename = "speed")]
    Speed,
    #[serde(rename = "seeking")]
    Seeking,
    #[serde(rename = "pause")]
    Pause,
}

#[derive(Debug)]
pub enum FloatProperty {
    TimePos,
    Speed,
}

#[derive(Debug, Deserialize)]
struct EventRaw {
    request_id: Option<i32>,
    error: Option<String>,
    event: Option<String>,
    name: Option<String>,
    data: Option<serde_json::Value>,
    args: Option<Vec<serde_json::Value>>,
}

#[derive(Debug)]
pub enum Event {
    SuccessResponse { request_id: i32 },
    ErrorResponse { request_id: i32, error: String },
    Event { event: String },
    ClientMessage { id: String },
    FloatPropertyChange { property: FloatProperty, value: f64 },
    Seek,
}

impl EventRaw {
    fn take_data_f64(&self) -> Result<f64, MpvError> {
        if let Some(ref value) = self.data {
            match value {
                serde_json::Value::Number(n) if n.is_f64() => Ok(n.as_f64().unwrap_or_default()),
                _ => Err(EventParsingError {
                    reason: "Expected .data to be float, got something else".to_string(),
                }
                .into()),
            }
        } else {
            // default values are ommited :/
            Ok(0.0)
        }
    }

    fn take_arg_str(&self) -> Result<String, MpvError> {
        if let Some(ref args) = self.args {
            if let Some(ref el) = args.first() {
                match el {
                    serde_json::Value::String(s) => Ok(s.clone()),
                    _ => Err(EventParsingError {
                        reason: "Expected .args[0] to be string, got something else".to_string(),
                    }
                    .into()),
                }
            } else {
                Err(EventParsingError {
                    reason: format!("Expected len(.args), got {}", args.len()).to_string(),
                }
                .into())
            }
        } else {
            Err(EventParsingError {
                reason: "Expected .args, got nothing".to_string(),
            }
            .into())
        }
    }

    fn parse(&self) -> Result<Option<Event>, MpvError> {
        let parsed_event = if let Some(ref error) = self.error {
            // branch one:
            Some(match error.as_str() {
                // if error field is set, it's either success
                "success" => Event::SuccessResponse {
                    request_id: self.request_id.unwrap_or_default(),
                },
                // or an actual error
                _ => Event::ErrorResponse {
                    request_id: self.request_id.unwrap_or_default(),
                    error: (*error).clone(),
                },
            })
        } else if let Some(ref event_type) = self.event {
            // branch two:
            match event_type.as_str() {
                // simple events which contain no properties
                "pause" | "unpause" | "playback-restart" => Some(Event::Event {
                    event: (*event_type).clone(),
                }),
                // client message
                "client-message" => Some(Event::ClientMessage {
                    id: self.take_arg_str()?,
                }),
                // float properties
                "property-change" => {
                    if let Some(ref name) = self.name {
                        match name.as_str() {
                            "time-pos" => Some(Event::FloatPropertyChange {
                                property: FloatProperty::TimePos,
                                value: self.take_data_f64()?,
                            }),
                            "speed" => Some(Event::FloatPropertyChange {
                                property: FloatProperty::Speed,
                                value: self.take_data_f64()?,
                            }),
                            // TODO: seeking bool
                            _ => None,
                        }
                    } else {
                        // other properties are ignored for now
                        None
                    }
                }
                "seek" => Some(Event::Seek),
                // ignore all others for now
                _ => {
                    warn!("mpv_parse_event: unsupported event_type {}", event_type);
                    None
                }
            }
        } else {
            None
        };

        Ok(parsed_event)
    }
}

impl Request {
    pub fn set_pause(val: bool) -> Request {
        Request {
            command: RequestType::SetProperty,
            v1: json!(Property::Pause),
            v2: json!(val),
        }
    }

    pub fn seek(val: f64) -> Request {
        Request {
            command: RequestType::SetProperty,
            v1: json!(Property::TimePos),
            v2: json!(val),
        }
    }

    pub fn speed(val: f64) -> Request {
        Request {
            command: RequestType::SetProperty,
            v1: json!(Property::Speed),
            v2: json!(val),
        }
    }

    pub fn observe_property(property: Property) -> Request {
        Request {
            command: RequestType::ObserveProperty,
            v1: json!(1),
            v2: json!(property),
        }
    }

    pub fn display_message(msg: &str) -> Request {
        Request {
            command: RequestType::OSDMsg,
            v1: json!("show-text"),
            v2: json!(msg),
        }
    }

    pub fn keybind(key: &str, command: &str) -> Request {
        Request {
            command: RequestType::Keybind,
            v1: json!(key),
            v2: json!(command),
        }
    }

    fn get_value(&self) -> serde_json::Value {
        json!({
            "command": json!(self),
        })
    }
}

pub async fn start(channels: &mut channels::MpvChannels) -> Result<(), MpvError> {
    let socket_path = {
        let tmp_dir = util::expand("$XDG_RUNTIME_DIR").unwrap_or("/tmp".to_string());
        let path_obj = std::path::Path::new(&tmp_dir);
        path_obj.join(format!("syncors-{}.socket", std::process::id()))
    };

    debug!("socket path: {}", socket_path.to_str().unwrap());

    let mut child = Command::new("mpv")
        .arg(format!(
            "--input-ipc-server={}",
            socket_path.to_str().unwrap()
        ))
        // .arg("--msg-level=all=debug")
        .arg("--pause")
        .arg(std::env::args().nth(1).unwrap())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let _d = defer(|| {
        std::fs::remove_file(socket_path.to_str().unwrap()).unwrap_or_default();
    });

    smol::future::or(
        synco_loop(&socket_path, channels),
        smol::future::or(wait_for_mpv(&mut child), wait_for_signal()),
    )
    .await
}

async fn wait_for_signal() -> Result<(), MpvError> {
    let mut signals = Signals::new(vec![libc::SIGINT]).unwrap();
    signals.next().await;
    Ok(())
}

async fn wait_for_mpv(child: &mut Child) -> Result<(), MpvError> {
    let exit_status = child.status().await?;

    match exit_status.code() {
        Some(code) if code != 0 => Err(ExitError {
            reason: "Non-zero exit code".to_string(),
            status_code: code,
        }
        .into()),
        _ => Ok(()),
    }
}

const CHECK_SOCKET_INTERVAL_MS: u64 = 100;
const CHECK_SOCKET_ATTEMPTS: i32 = 10;

async fn synco_loop(
    socket_path: &PathBuf,
    channels: &mut channels::MpvChannels,
) -> Result<(), MpvError> {
    // first: waiting for socket
    for _ in 1..CHECK_SOCKET_ATTEMPTS {
        if socket_path.exists() {
            break;
        }

        Timer::after(Duration::from_millis(CHECK_SOCKET_INTERVAL_MS)).await;
    }

    if !socket_path.exists() {
        return Err(MpvStartupError {
            reason: "Waited too long for mpv socket to appear".to_string(),
        }
        .into());
    }

    let stream = UnixStream::connect(socket_path).await?;

    let (reader, mut writer) = smol::io::split(stream);

    or(
        read_loop(reader, &channels.from_mpv_send),
        write_loop(&mut writer, &mut channels.to_mpv_receive),
    )
    .await
}

async fn read_loop(reader: ReadHalf<UnixStream>, msg_send: &Sender<Event>) -> Result<(), MpvError> {
    let reader = BufReader::new(reader);

    // just read mpv commands
    let mut lines = reader.lines();
    while let Some(line) = lines.next().await {
        let line = line?;
        let event: EventRaw = serde_json::from_str(&line)?;

        // some events may be skipped for now
        if let Some(parsed_event) = event.parse()? {
            debug!("real_loop parsed_event: {:?}", parsed_event);
            msg_send.send(parsed_event).await?;
        }
    }

    Ok(())
}

async fn write_loop(
    writer: &mut WriteHalf<UnixStream>,
    msg_receive: &mut Receiver<Request>,
) -> Result<(), MpvError> {
    // just write mpv commands
    while let Some(ref msg) = msg_receive.next().await {
        let mut to_write = serde_json::to_vec(&msg.get_value())?;

        debug!(
            "write_loop next line: {}",
            String::from_utf8(to_write.clone()).unwrap()
        );

        to_write.push(b'\n');

        writer.write(&to_write).await?;
    }

    Ok(())
}
