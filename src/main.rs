use std::env;
use std::error::Error;
use std::net::SocketAddr;

use futures_util::{StreamExt, TryStreamExt, stream};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, BodyStream};
use http_body_util::{Full, StreamBody};
use hyper::body::{Bytes, Frame};
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use indicatif::{ProgressBar, ProgressStyle};
use multer::Multipart;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use qrcode::QrCode;
use qrcode::render::{svg, unicode};
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

const BAR_STYLE: &str = "[{elapsed_precise}] {msg} {wide_bar:.cyan/blue} {decimal_bytes} / {decimal_total_bytes} ({decimal_bytes_per_sec}) ";
const SPINNER_STYLE: &str =
    "[{elapsed_precise}] {spinner} {msg} : {decimal_bytes} ({decimal_bytes_per_sec})";

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
        let is_root = path == "/";
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
                let bar = if let Some(length) = req
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|length| length.parse().ok())
                {
                    ProgressBar::new(length)
                        .with_style(ProgressStyle::default_bar().template(BAR_STYLE)?)
                } else {
                    ProgressBar::new_spinner()
                        .with_style(ProgressStyle::default_spinner().template(SPINNER_STYLE)?)
                };
                let mut output = Vec::new();
                let body_stream = BodyStream::new(req.into_body()).filter_map(|result| {
                    let bar = bar.clone();
                    async move {
                        result
                            .map(|frame| {
                                frame.into_data().ok().map(|data| {
                                    bar.inc(data.len() as u64);
                                    data
                                })
                            })
                            .transpose()
                    }
                });
                let mut multipart = Multipart::new(body_stream, boundary);
                while let Some(mut field) = multipart.next_field().await? {
                    let Some("file") = field.name() else {
                        continue;
                    };
                    let file_name = field
                        .file_name()
                        .unwrap_or("unnamed file uploaded with hotspot drop");
                    bar.set_message(String::from(file_name));
                    let path = format!("{}/{file_name}", filename.to_string_lossy());
                    let mut file = File::create(path.clone()).await?;

                    let mut field_bytes_len = 0;
                    while let Some(field_chunk) = field.chunk().await? {
                        file.write_all(&field_chunk).await?;
                        field_bytes_len += field_chunk.len();
                    }
                    output.push((path, field_bytes_len));
                }
                bar.finish_and_clear();
                for (path, size) in output {
                    println!("W {path} ({size} bytes)");
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
        println!("R {}", filename.to_str().unwrap_or(""));
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
        let folder_path = {
            let mut path = filename.strip_prefix(cwd)?.to_string_lossy().into_owned();
            if !path.starts_with('/') {
                path.insert(0, '/');
            }
            if !path.ends_with('/') {
                path.push('/');
            }
            path
        };
        let mut folder_listing = paths
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
            .join("");
        if !is_root {
            folder_listing = format!(
                r#"<li><a href="../"><span class="spacer">Parent folder</span> ../</a></li>{folder_listing}"#
            );
        }
        let qr_codes = if is_root {
            NetworkInterface::show()
                .unwrap()
                .iter()
                .flat_map(|itf| itf.addr.iter())
                .filter_map(|addr| {
                    if let Addr::V4(addr) = addr {
                        Some(format!("http://{}:{PORT}", addr.ip))
                    } else {
                        None
                    }
                })
                .map(|url| {
                    let qrcode = QrCode::new(&url).unwrap();
                    format!(
                        r#"<div class="qr-code">{}<a href="{url}">{url}</a></div>"#,
                        qrcode.render::<svg::Color>().build(),
                    )
                })
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };
        Ok(Response::builder().status(StatusCode::OK).body(
            Full::new(
                include_str!("./index.html")
                    .replace("{PATH}", &folder_path)
                    .replace("{LIST}", &folder_listing)
                    .replace("{QR}", &qr_codes)
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

        println!("R {}", filename.to_str().unwrap_or(""));
        let file = File::open(filename.clone()).await?;
        let size = file.metadata().await?.len().try_into()?;
        let bar =
            ProgressBar::new(size).with_style(ProgressStyle::default_bar().template(BAR_STYLE)?);
        let reader_stream = ReaderStream::new(file);
        let stream_body = StreamBody::new(
            reader_stream
                .map_ok({
                    let bar = bar.clone();
                    move |file| {
                        Frame::data(file).map_data({
                            let bar = bar.clone();
                            move |data| {
                                bar.inc(data.len() as u64);
                                data
                            }
                        })
                    }
                })
                .chain(stream::once({
                    let bar = bar.clone();
                    async move {
                        bar.finish_and_clear();
                        Ok(Frame::data(Bytes::new()))
                    }
                })),
        );
        let boxed_body = BodyExt::boxed(stream_body);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(
                CONTENT_TYPE,
                mime_guess::from_path(filename).first().map_or_else(
                    || String::from("application/octet-stream"),
                    |mime| mime.to_string(),
                ),
            )
            .header(CONTENT_LENGTH, size.to_string())
            .body(boxed_body)?)
    }
}

const PORT: u16 = 6969;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
    for itf in NetworkInterface::show().unwrap().iter() {
        for addr in &itf.addr {
            if let Addr::V4(addr) = addr {
                let url = format!("http://{}:{PORT}", addr.ip);
                let qrcode = QrCode::new(&url).unwrap();
                println!("{url}");
                println!(
                    "{}",
                    qrcode
                        .render::<unicode::Dense1x2>()
                        .quiet_zone(false)
                        .build()
                );
                println!();
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
