// /opt/homebrew/opt/curl/bin/curl --http3-only -k -v https://localhost:8443 \
// -d "data"

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};

use bytes::{Buf, Bytes, BytesMut};
use h3::server::{Connection, RequestResolver};
use http::{HeaderMap, Method};
use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tracing::{error, info, warn};

static ALPN: &[u8] = b"h3";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let provider = rustls::crypto::ring::default_provider();
    provider.install_default().unwrap();

    let csrt_path = "/Users/nakayaryo/develop/rs_sbx/ssl/server.der.crt";
    let key_path = "/Users/nakayaryo/develop/rs_sbx/ssl/server.der.key";

    let csrt = CertificateDer::from(std::fs::read(csrt_path)?);
    let key = PrivateKeyDer::try_from(std::fs::read(key_path)?)?;

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![csrt], key)?;
    tls_config.max_early_data_size = u32::MAX;
    tls_config.alpn_protocols = vec![ALPN.into()];

    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 8443));
    let server_config =
        quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls_config)?));
    let endpoint = quinn::Endpoint::server(server_config, addr)?;

    info!("listening on {}", addr);
    while let Some(conn) = endpoint.accept().await {
        info!("-----New Connection-----");
        tokio::task::spawn(async move {
            match conn.await {
                Ok(conn) => {
                    let mut h3_conn: Connection<_, bytes::Bytes> =
                        h3::server::Connection::new(h3_quinn::Connection::new(conn))
                            .await
                            .unwrap();
                    info!("established h3 conn");

                    loop {
                        match h3_conn.accept().await {
                            Ok(Some(v)) => {
                                info!("handle request");
                                tokio::task::spawn(async move {
                                    if handle_request(v).await.is_err() {
                                        error!("error while handling request");
                                    }
                                });
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(e) => {
                                match e {
                                    h3::error::ConnectionError::Timeout => warn!("Time Out"),
                                    _ => error!("{:?}", e),
                                }
                                break;
                            }
                        }
                    }
                    info!("-----Connection Closed-----");
                }
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });
    }
    Ok(())
}

async fn handle_request<C>(
    resolver: RequestResolver<C, Bytes>,
) -> Result<(), Box<dyn std::error::Error>>
where
    C: h3::quic::Connection<Bytes>,
{
    let (req, mut stream) = resolver.resolve_request().await?;
    info!("{:?}", req);

    let mut body = BytesMut::new();
    if req.method() == Method::POST {
        while let Some(chunk) = stream.recv_data().await? {
            body.extend_from_slice(chunk.chunk());
        }
    } else {
        body = BytesMut::from("Empty");
    }

    let resp = http::Response::builder().status(200).body(()).unwrap();
    stream.send_response(resp).await?;

    for _ in 0..10 {
        let b = body.clone();
        stream.send_data(b.into()).await?;
    }

    let header_map = HeaderMap::default();
    if let Err(e) = stream.send_trailers(header_map).await {
        error!("error to send trailers: {:?}", e);
    }
    Ok(stream.finish().await?)
}
