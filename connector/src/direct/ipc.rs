//! Authenticated same-machine IPC. Credentials never appear in RPC replies.
use super::{LocalReply, LocalRequest};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(thiserror::Error, Debug)]
pub enum IpcError {
    #[error("local agent IPC: {0}")]
    Io(#[from] io::Error),
    #[error("invalid local agent payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("local agent authentication failed")]
    Authentication,
}
#[derive(Serialize, Deserialize)]
struct Endpoint {
    name: String,
    token: String,
    instance: String,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    request: LocalRequest,
}
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub struct Connection {
    stream: Box<dyn Stream>,
    token: String,
}
// A view contains up to 64 messages, including JSON escaping of long text.
const MAX_FRAME: usize = 104 * 1024 * 1024;
async fn read<T: serde::de::DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin + ?Sized),
) -> Result<T, IpcError> {
    let size = stream.read_u32().await? as usize;
    if size > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "IPC frame too large").into());
    }
    let mut data = vec![0; size];
    stream.read_exact(&mut data).await?;
    Ok(serde_json::from_slice(&data)?)
}
async fn write<T: Serialize>(
    stream: &mut (impl AsyncWrite + Unpin + ?Sized),
    value: &T,
) -> Result<(), IpcError> {
    let data = serde_json::to_vec(value)?;
    if data.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "IPC frame too large").into());
    }
    stream.write_u32(data.len() as u32).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}
impl Connection {
    pub async fn receive(&mut self) -> Result<LocalRequest, IpcError> {
        let envelope: Envelope = read(&mut *self.stream).await?;
        if envelope.token != self.token {
            return Err(IpcError::Authentication);
        }
        Ok(envelope.request)
    }
    pub async fn respond(&mut self, reply: &LocalReply) -> Result<(), IpcError> {
        write(&mut *self.stream, reply).await
    }
}
pub async fn call(directory: &Path, request: &LocalRequest) -> Result<LocalReply, IpcError> {
    let endpoint: Endpoint =
        serde_json::from_slice(&tokio::fs::read(directory.join("direct-endpoint.json")).await?)?;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(&endpoint.name)?;
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(&endpoint.name).await?;
    let mut stream: Box<dyn Stream> = Box::new(stream);
    write(
        &mut *stream,
        &Envelope {
            token: endpoint.token,
            request: request.clone(),
        },
    )
    .await?;
    read(&mut *stream).await
}
pub struct Listener {
    endpoint: Endpoint,
    path: PathBuf,
    #[cfg(windows)]
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    #[cfg(unix)]
    server: tokio::net::UnixListener,
    #[cfg(unix)]
    _socket_directory: tempfile::TempDir,
}
impl Listener {
    pub fn bind(directory: &Path) -> Result<Self, IpcError> {
        let instance = uuid::Uuid::new_v4().to_string();
        #[cfg(windows)]
        let name = format!(r"\\.\pipe\llmn-direct-{instance}");
        #[cfg(unix)]
        let socket_directory = {
            use std::os::unix::fs::PermissionsExt;
            // Node directories contain server/account IDs and exceed sockaddr_un's limit.
            // Keep only the socket in a short, private directory; the credential stays in
            // the protected node descriptor. Explicit permissions avoid relying on umask.
            tempfile::Builder::new()
                .prefix("llmn-direct-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in("/tmp")?
        };
        #[cfg(unix)]
        let name = socket_directory
            .path()
            .join("rpc.sock")
            .to_string_lossy()
            .into_owned();
        #[cfg(windows)]
        let server = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(&name)?;
        #[cfg(unix)]
        let server = tokio::net::UnixListener::bind(&name)?;
        let endpoint = Endpoint {
            name,
            instance,
            token: format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()),
        };
        let path = directory.join("direct-endpoint.json");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        std::io::Write::write_all(&mut file, &serde_json::to_vec(&endpoint)?)?;
        Ok(Self {
            endpoint,
            path,
            server,
            #[cfg(unix)]
            _socket_directory: socket_directory,
        })
    }
    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        #[cfg(windows)]
        let stream = {
            self.server.connect().await?;
            let next = tokio::net::windows::named_pipe::ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.endpoint.name)?;
            std::mem::replace(&mut self.server, next)
        };
        #[cfg(unix)]
        let stream = self.server.accept().await?.0;
        Ok(Connection {
            stream: Box::new(stream),
            token: self.endpoint.token.clone(),
        })
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.endpoint.name);
        }
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn socket_stays_short_and_private_for_long_node_directory() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("a".repeat(64)).join("b".repeat(64));
        std::fs::create_dir_all(&directory).unwrap();
        let listener = Listener::bind(&directory).unwrap();
        let socket = PathBuf::from(&listener.endpoint.name);
        let socket_directory = socket.parent().unwrap().to_owned();
        assert!(listener.endpoint.name.len() < 108);
        assert_eq!(
            std::fs::metadata(&socket_directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert!(directory.join("direct-endpoint.json").exists());
        drop(listener);
        assert!(!socket_directory.exists());
        assert!(!directory.join("direct-endpoint.json").exists());
    }
}
