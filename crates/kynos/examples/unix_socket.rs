//! Serving on a Unix domain socket, behind a local proxy or sidecar.
//!
//! ```text
//! cargo run -p kynos --example unix_socket
//! curl --unix-socket "${TMPDIR:-/tmp}/kynos-example.sock" http://localhost/hello
//! ```
//!
//! Set `KYNOS_SOCKET` to serve on another path. Three things are worth
//! noticing:
//!
//! * **The socket file is yours, start to finish.** Kynos takes a listener
//!   already bound and never touches the path: removing a stale file before
//!   binding, choosing who may connect, and removing the file afterwards are
//!   each a decision about this deployment, so each is made below rather than
//!   guessed at by the framework. A stale file is only removed when it is a
//!   socket, so a typo never deletes someone's data.
//! * **A Unix listener is a listener.** The connection limit, graceful
//!   shutdown and its drain deadline apply exactly as they do to TCP; TCP
//!   keepalive does not, and TLS is refused, since a peer on the same host is
//!   reached without crossing a network.
//! * **The peer is a Unix address, not an IP one.** A connecting client is
//!   usually unnamed, so a handler reads it through `Connection::peer`;
//!   `ConnectInfo` promises an IP peer and answers its declared 500 here.

#[cfg(unix)]
use kynos::{
    extract::{
        body::text::Text,
        connection::{Connection, TransportAddress},
    },
    prelude::*,
    server::{Server, shutdown::Shutdown},
};

/// Greets whoever connected, by the transport they connected over.
#[cfg(unix)]
#[kynos::get("/hello")]
async fn hello(connection: Connection) -> Text {
    Text(match connection.peer() {
        TransportAddress::Unix(peer) => match peer.as_std().as_pathname() {
            Some(path) => format!("hello, {}\n", path.display()),
            None => "hello, unnamed Unix peer\n".to_owned(),
        },
        TransportAddress::Ip(peer) => format!("hello, {peer}\n"),
        _ => "hello, stranger\n".to_owned(),
    })
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        os::unix::fs::{FileTypeExt as _, PermissionsExt as _},
        path::PathBuf,
    };

    let path = std::env::var_os("KYNOS_SOCKET").map_or_else(
        || std::env::temp_dir().join("kynos-example.sock"),
        PathBuf::from,
    );

    // A socket left behind by a process that did not exit cleanly refuses the
    // bind, so remove it; anything else at the path is not ours to remove.
    if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
        std::fs::remove_file(&path)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    // Connecting needs write permission on the file: the owner and its group.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))?;

    let service = Router::<()>::new().mount(kynos::routes![hello]).build(())?;
    println!("listening on {}", path.display());
    let served = Server::new(service)
        .unix_listener(listener)
        .graceful_shutdown(Shutdown::signals())
        .serve()
        .await;

    // Kynos leaves the file in place when it stops.
    std::fs::remove_file(&path)?;
    match served {
        Err(kynos::Error::Server(error)) if error.is_requested_shutdown() => Ok(()),
        served => Ok(served?),
    }
}

/// A Unix domain socket is what this example serves on.
#[cfg(not(unix))]
fn main() {
    eprintln!("this example serves on a Unix domain socket, which this platform lacks");
}
