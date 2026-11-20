use crate::protocol::{Message, Scene};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

const TICK: Duration = Duration::from_millis(20);
const MAX_LINE: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Join,
}

#[derive(Debug)]
pub enum Event {
    State(Scene),
    Status { text: String, connected: bool },
}

pub struct Network {
    commands: Sender<Message>,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Network {
    pub fn start(role: Role, path: PathBuf, proxy: EventLoopProxy<()>) -> Self {
        let (commands, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let handle = thread::spawn(move || {
            let result = match role {
                Role::Host => run_host(path, rx, &tx, &proxy),
                Role::Join => run_join(path, rx, &tx, &proxy),
            };
            if let Err(error) = result {
                emit(
                    &tx,
                    &proxy,
                    Event::Status {
                        text: error.to_string(),
                        connected: false,
                    },
                );
            }
        });
        Self {
            commands,
            events,
            handle: Some(handle),
        }
    }

    pub fn send(&self, message: Message) {
        let _ = self.commands.send(message);
    }

    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        // Closing the channel ends the worker on its next tick.
        let (replacement, _) = mpsc::channel();
        self.commands = replacement;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn emit(tx: &Sender<Event>, proxy: &EventLoopProxy<()>, event: Event) {
    let _ = tx.send(event);
    let _ = proxy.send_event(());
}

fn prepare_socket(path: &PathBuf) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if !fs::symlink_metadata(path)?.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "socket path exists and is not a socket",
        ));
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "another host is already listening",
        )),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(path),
        Err(error) => Err(error),
    }
}

struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn setup_stream(stream: &UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(TICK))?;
    stream.set_write_timeout(Some(Duration::from_millis(200)))?;
    Ok(())
}

fn send(stream: &mut UnixStream, message: &Message) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)
}

fn receive(stream: &mut UnixStream, pending: &mut Vec<u8>) -> io::Result<Option<Message>> {
    if let Some(index) = pending.iter().position(|&b| b == b'\n') {
        let line: Vec<_> = pending.drain(..=index).collect();
        return serde_json::from_slice(&line)
            .map(Some)
            .map_err(io::Error::other);
    }
    let mut chunk = [0u8; 1024];
    match stream.read(&mut chunk) {
        Ok(0) => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "peer disconnected",
        )),
        Ok(n) => {
            pending.extend_from_slice(&chunk[..n]);
            if pending.len() > MAX_LINE {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message too long",
                ));
            }
            if let Some(index) = pending.iter().position(|&b| b == b'\n') {
                let line: Vec<_> = pending.drain(..=index).collect();
                serde_json::from_slice(&line)
                    .map(Some)
                    .map_err(io::Error::other)
            } else {
                Ok(None)
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn run_host(
    path: PathBuf,
    rx: Receiver<Message>,
    tx: &Sender<Event>,
    proxy: &EventLoopProxy<()>,
) -> io::Result<()> {
    prepare_socket(&path)?;
    let listener = UnixListener::bind(&path)?;
    let _guard = SocketGuard(path.clone());
    listener.set_nonblocking(true)?;
    let mut scene = Scene::default();
    let mut peer: Option<(UnixStream, Vec<u8>)> = None;
    emit(
        tx,
        proxy,
        Event::Status {
            text: format!("Listening: {}", path.display()),
            connected: false,
        },
    );

    loop {
        loop {
            match rx.try_recv() {
                Ok(message) => {
                    if scene.apply(&message) {
                        emit(tx, proxy, Event::State(scene.clone()));
                        if let Some((stream, _)) = &mut peer
                            && send(
                                stream,
                                &Message::State {
                                    scene: scene.clone(),
                                },
                            )
                            .is_err()
                        {
                            peer = None;
                            emit(
                                tx,
                                proxy,
                                Event::Status {
                                    text: "Peer disconnected".into(),
                                    connected: false,
                                },
                            );
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        if peer.is_none() {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    setup_stream(&stream)?;
                    if send(
                        &mut stream,
                        &Message::State {
                            scene: scene.clone(),
                        },
                    )
                    .is_ok()
                    {
                        peer = Some((stream, Vec::new()));
                        emit(
                            tx,
                            proxy,
                            Event::Status {
                                text: "Peer connected".into(),
                                connected: true,
                            },
                        );
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(TICK),
                Err(error) => return Err(error),
            }
            continue;
        }

        let received = {
            let (stream, pending) = peer.as_mut().unwrap();
            receive(stream, pending)
        };
        match received {
            Ok(Some(message)) => {
                if scene.apply(&message) {
                    emit(tx, proxy, Event::State(scene.clone()));
                    if let Some((stream, _)) = &mut peer
                        && send(
                            stream,
                            &Message::State {
                                scene: scene.clone(),
                            },
                        )
                        .is_err()
                    {
                        peer = None;
                        emit(
                            tx,
                            proxy,
                            Event::Status {
                                text: "Peer disconnected".into(),
                                connected: false,
                            },
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(_) => {
                peer = None;
                emit(
                    tx,
                    proxy,
                    Event::Status {
                        text: "Peer disconnected; listening".into(),
                        connected: false,
                    },
                );
            }
        }
    }
}

fn run_join(
    path: PathBuf,
    rx: Receiver<Message>,
    tx: &Sender<Event>,
    proxy: &EventLoopProxy<()>,
) -> io::Result<()> {
    let mut peer: Option<(UnixStream, Vec<u8>)> = None;
    emit(
        tx,
        proxy,
        Event::Status {
            text: format!("Connecting: {}", path.display()),
            connected: false,
        },
    );
    loop {
        if peer.is_none() {
            match UnixStream::connect(&path) {
                Ok(stream) => {
                    setup_stream(&stream)?;
                    peer = Some((stream, Vec::new()));
                    emit(
                        tx,
                        proxy,
                        Event::Status {
                            text: "Connected to host".into(),
                            connected: true,
                        },
                    );
                }
                Err(_) => {
                    match rx.try_recv() {
                        Err(TryRecvError::Disconnected) => return Ok(()),
                        Ok(_) | Err(TryRecvError::Empty) => {}
                    }
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            }
        }

        loop {
            match rx.try_recv() {
                Ok(message) => {
                    if let Some((stream, _)) = &mut peer
                        && send(stream, &message).is_err()
                    {
                        peer = None;
                        emit(
                            tx,
                            proxy,
                            Event::Status {
                                text: "Disconnected; retrying".into(),
                                connected: false,
                            },
                        );
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some((stream, pending)) = &mut peer {
            match receive(stream, pending) {
                Ok(Some(Message::State { scene })) => emit(tx, proxy, Event::State(scene)),
                Ok(_) => {}
                Err(_) => {
                    peer = None;
                    emit(
                        tx,
                        proxy,
                        Event::Status {
                            text: "Disconnected; retrying".into(),
                            connected: false,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_handles_partial_and_multiple_messages() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let first = Message::SetScale { value: 1.5 };
        let second = Message::SetColor {
            value: [0.1, 0.2, 0.3],
        };
        let mut bytes = serde_json::to_vec(&first).unwrap();
        bytes.push(b'\n');
        bytes.extend(serde_json::to_vec(&second).unwrap());
        bytes.push(b'\n');
        let split = 8;
        writer.write_all(&bytes[..split]).unwrap();
        let mut pending = Vec::new();
        assert_eq!(receive(&mut reader, &mut pending).unwrap(), None);
        writer.write_all(&bytes[split..]).unwrap();
        assert_eq!(receive(&mut reader, &mut pending).unwrap(), Some(first));
        assert_eq!(receive(&mut reader, &mut pending).unwrap(), Some(second));
    }
}
