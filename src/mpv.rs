use crate::util;
use async_channel::{Receiver, SendError, Sender};
use async_process::{Child, Command, Stdio};
use async_signals::Signals;
use futures_lite::future::or;
use futures_lite::io::{AsyncBufReadExt, BufReader, ReadHalf, WriteHalf};
use futures_lite::prelude::*;
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
        SendError(err: SendError<String>) {
            from()
        }
    }
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

pub type Message = String;

pub async fn start(
    msg_receive: &mut Receiver<Message>,
    msg_send: Sender<Message>,
) -> Result<(), MpvError> {
    let socket_path = {
        let tmp_dir = util::expand("$XDG_RUNTIME_DIR").unwrap_or("/tmp".to_string());
        let path_obj = std::path::Path::new(&tmp_dir);
        path_obj.join(format!("syncors-{}.socket", std::process::id()))
    };

    println!("socket path: {}", socket_path.to_str().unwrap());

    let mut child = Command::new("mpv")
        .arg(format!(
            "--input-ipc-server={}",
            socket_path.to_str().unwrap()
        ))
        .arg("--pause")
        .arg(std::env::args().nth(1).unwrap())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    smol::future::or(
        synco_loop(&socket_path, msg_receive, msg_send),
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
    msg_receive: &mut Receiver<Message>,
    msg_send: Sender<Message>,
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
        read_loop(reader, msg_send),
        write_loop(&mut writer, msg_receive),
    )
    .await
}

async fn read_loop(
    reader: ReadHalf<UnixStream>,
    msg_send: Sender<Message>,
) -> Result<(), MpvError> {
    let reader = BufReader::new(reader);

    // just read mpv commands
    let mut lines = reader.lines();
    while let Some(line) = lines.next().await {
        let line = line?;
        println!("mpv got line: {}", line);
        msg_send.send(line).await?;
    }

    Ok(())
}

async fn write_loop(
    writer: &mut WriteHalf<UnixStream>,
    msg_receive: &mut Receiver<Message>,
) -> Result<(), MpvError> {
    // just write mpv commands
    while let Some(msg) = msg_receive.next().await {
        writer.write(msg.as_bytes()).await?;
    }

    Ok(())
}
