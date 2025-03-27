use std::env;
use std::error::Error;
use std::net::SocketAddr;

use futures_util::{StreamExt, TryStreamExt};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, BodyStream};
use http_body_util::{Full, StreamBody};
use hyper::body::{Bytes, Frame};
use hyper::header::CONTENT_TYPE;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use multer::Multipart;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

fn escape_html(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, std::io::Error>>, Box<dyn Error + Send + Sync>> {
    let path = req.uri().path();
    let url_decoded = format!(".{}", urlencoding::decode(path)?);
    let cwd = env::current_dir()?.canonicalize()?;
    let Ok(filename) = cwd.join(&url_decoded).canonicalize() else {
        return Ok(Response::builder().status(StatusCode::NOT_FOUND).body(
            Full::new(Bytes::from("404 Not Found :("))
                .map_err(|e| match e {})
                .boxed(),
        )?);
    };
    println!("R {}", filename.to_str().unwrap_or(""));
    if !filename.starts_with(cwd.clone()) {
        return Ok(Response::builder()
            .status(StatusCode::UNPROCESSABLE_ENTITY)
            .body(
                Full::new(Bytes::from(
                    "422 Unprocessable Entity: directory traversal attempt thwarted hehe",
                ))
                .map_err(|e| match e {})
                .boxed(),
            )?);
    }
    let Ok(metadata) = fs::metadata(filename.clone()).await else {
        return Ok(Response::builder().status(StatusCode::NOT_FOUND).body(
            Full::new(Bytes::from("404 Not Found :("))
                .map_err(|e| match e {})
                .boxed(),
        )?);
    };
    if metadata.is_dir() {
        let path_ends_with_slash = if path.ends_with('/') {
            None
        } else {
            Some(format!("{}/", path))
        };
        match req.method() {
            &Method::GET => {}
            &Method::POST => {
                // https://github.com/rwf2/multer/blob/master/examples/hyper_server_example.rs
                let Some(boundary) = req
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|ct| ct.to_str().ok())
                    .and_then(|ct| multer::parse_boundary(ct).ok())
                else {
                    return Ok(Response::builder().status(StatusCode::BAD_REQUEST).body(
                        Full::from("400 Bad Request: missing multipart boundary >:(")
                            .map_err(|e| match e {})
                            .boxed(),
                    )?);
                };
                let body_stream =
                    BodyStream::new(req.into_body()).filter_map(|result| async move {
                        result.map(|frame| frame.into_data().ok()).transpose()
                    });
                let mut multipart = Multipart::new(body_stream, boundary);
                while let Some(mut field) = multipart.next_field().await? {
                    let Some("file") = field.name() else {
                        continue;
                    };
                    let file_name = field
                        .file_name()
                        .unwrap_or("unnamed file uploaded with hotspot drop");
                    let path = format!("{}/{file_name}", filename.to_string_lossy());
                    let mut file = File::create(path.clone()).await?;

                    let mut field_bytes_len = 0;
                    while let Some(field_chunk) = field.chunk().await? {
                        file.write_all(&field_chunk).await?;
                        field_bytes_len += field_chunk.len();
                    }
                    println!("W {path} ({:?} bytes)", field_bytes_len);
                }
            }
            _ => {
                return Ok(Response::builder()
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .body(
                        Full::new(Bytes::from("405 Method Not Allowed :("))
                            .map_err(|e| match e {})
                            .boxed(),
                    )?);
            }
        }

        if let Some(new_path) = path_ends_with_slash {
            return Ok(Response::builder()
                .status(301)
                .header("Location", new_path)
                .body(
                    Full::new(Bytes::from("Redirecting to directory listing..."))
                        .map_err(|e| match e {})
                        .boxed(),
                )?);
        }
        let mut paths = vec![];
        let mut entries = fs::read_dir(filename.clone()).await?;
        while let Some(entry) = entries.next_entry().await? {
            match entry.file_name().into_string() {
                Ok(name) => paths.push((name, entry.metadata().await?)),
                Err(name) => eprintln!("Failed to decode '{name:?}'"),
            }
        }
        paths.sort_by(|a, b| {
            a.1.is_file()
                .cmp(&b.1.is_file())
                .then_with(|| a.0.cmp(&b.0))
        });
        Ok(Response::builder().status(StatusCode::OK).body(
            Full::new(
                include_str!("./index.html")
                    .replace("{PATH}", &{
                        let mut path = filename.strip_prefix(cwd)?.to_string_lossy().into_owned();
                        if !path.starts_with('/') {
                            path.insert(0, '/');
                        }
                        if !path.ends_with('/') {
                            path.push('/');
                        }
                        path
                    })
                    .replace(
                        "{LIST}",
                        &paths
                            .iter()
                            .map(|(name, metadata)| {
                                if metadata.is_dir() {
                                    format!(
                                        r#"<li><a href="{0}/"><span class="folder">Folder</span> {0}/</a></li>"#,
                                        escape_html(name)
                                    )
                                } else {
                                    format!(
                                        r#"<li><a href="{0}"><span class="file">File</span> {0}</a> <a href="{0}" download class="download">Download {0}</a></li>"#,
                                        escape_html(name)
                                    )
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(""),
                    )
                    .into(),
            )
            .map_err(|e| match e {})
            .boxed(),
        )?)
    } else {
        match req.method() {
            &Method::GET => {}
            _ => {
                return Ok(Response::builder()
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .body(
                        Full::new(Bytes::from("405 Method Not Allowed :("))
                            .map_err(|e| match e {})
                            .boxed(),
                    )?);
            }
        }

        let file = File::open(filename.clone()).await?;
        let reader_stream = ReaderStream::new(file);
        let stream_body = StreamBody::new(reader_stream.map_ok(Frame::data));
        let boxed_body = BodyExt::boxed(stream_body);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                "Content-Type",
                mime_guess::from_path(filename).first().map_or_else(
                    || String::from("application/octet-stream"),
                    |mime| mime.to_string(),
                ),
            )
            .body(boxed_body)?)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = 6969;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    for itf in NetworkInterface::show().unwrap().iter() {
        for addr in &itf.addr {
            if let Addr::V4(addr) = addr {
                println!("http://{}:{port}", addr.ip);
            }
        }
    }

    // We create a TcpListener and bind it to 127.0.0.1:3000
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(handle_request))
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}
