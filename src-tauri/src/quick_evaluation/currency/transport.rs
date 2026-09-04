use std::{fmt, future::Future, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{
    client::conn::http1,
    header::{ACCEPT, CONNECTION, HOST},
    Method, Request,
};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    task::JoinHandle,
};
use tokio_rustls::rustls::{pki_types::ServerName, ClientConfig, RootCertStore};

use super::{CurrencyCode, ExchangeRateRequest};

pub(super) const PROVIDER_HOST: &str = "api.frankfurter.dev";
pub(super) const PROVIDER_PORT: u16 = 443;
pub(super) const PROVIDER_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_HEADER_FIELDS: usize = 64;
const MAX_RESPONSE_HEADER_BYTES: usize = 32 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ProviderHttpRequest {
    method: &'static str,
    path_and_query: String,
    headers: Vec<(String, Vec<u8>)>,
}

impl fmt::Debug for ProviderHttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderHttpRequest")
            .field("redacted", &true)
            .finish()
    }
}

impl ProviderHttpRequest {
    pub(super) fn for_exchange(request: &ExchangeRateRequest) -> Option<Self> {
        if !canonical_code(&request.base) || !canonical_code(&request.quote) {
            return None;
        }
        Some(Self {
            method: "GET",
            path_and_query: format!(
                "/v2/rate/{}/{}",
                request.base.as_str(),
                request.quote.as_str()
            ),
            headers: vec![
                ("host".to_owned(), PROVIDER_HOST.as_bytes().to_vec()),
                ("accept".to_owned(), b"application/json".to_vec()),
            ],
        })
    }

    pub(crate) fn method(&self) -> &str {
        self.method
    }

    pub(crate) fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    pub(crate) fn headers(&self) -> &[(String, Vec<u8>)] {
        &self.headers
    }

    fn is_exact(&self) -> bool {
        let Some(pair) = self.path_and_query.strip_prefix("/v2/rate/") else {
            return false;
        };
        let Some((base, quote)) = pair.split_once('/') else {
            return false;
        };
        self.method == "GET"
            && base.len() == 3
            && quote.len() == 3
            && base.bytes().all(|byte| byte.is_ascii_uppercase())
            && quote.bytes().all(|byte| byte.is_ascii_uppercase())
            && self.headers
                == [
                    ("host".to_owned(), PROVIDER_HOST.as_bytes().to_vec()),
                    ("accept".to_owned(), b"application/json".to_vec()),
                ]
    }
}

fn canonical_code(code: &CurrencyCode) -> bool {
    code.as_str().len() == 3
        && code.as_str().bytes().all(|byte| byte.is_ascii_uppercase())
        && CurrencyCode::new(code.as_str()).as_ref() == Some(code)
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ProviderHttpResponse {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

impl fmt::Debug for ProviderHttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderHttpResponse")
            .field("status", &self.status)
            .field("redacted", &true)
            .finish()
    }
}

impl ProviderHttpResponse {
    pub(crate) fn try_new(
        status: u16,
        headers: Vec<(String, Vec<u8>)>,
        body: Vec<u8>,
    ) -> Result<Self, ProviderTransportError> {
        let header_bytes = headers.iter().try_fold(0_usize, |total, (name, value)| {
            total.checked_add(name.len())?.checked_add(value.len())
        });
        if headers.len() > MAX_RESPONSE_HEADER_FIELDS
            || header_bytes.is_none_or(|bytes| bytes > MAX_RESPONSE_HEADER_BYTES)
            || body.len() > MAX_RESPONSE_BODY_BYTES
        {
            return Err(ProviderTransportError::ResponseTooLarge);
        }
        Ok(Self {
            status,
            headers,
            body,
        })
    }

    pub(crate) fn status(&self) -> u16 {
        self.status
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn headers(&self) -> &[(String, Vec<u8>)] {
        &self.headers
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderTransportError {
    Unavailable,
    ResponseTooLarge,
}

pub(crate) type ProviderTransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderHttpResponse, ProviderTransportError>> + Send + 'a>>;

pub(crate) trait ProviderTransport: Send + Sync {
    fn execute<'a>(&'a self, request: ProviderHttpRequest) -> ProviderTransportFuture<'a>;
}

pub(super) trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

pub(super) type BoxedIo = Box<dyn AsyncIo>;

pub(super) type ProviderDnsFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, ProviderTransportError>> + Send + 'a>>;

pub(super) trait ProviderDnsResolver: Send + Sync {
    fn resolve<'a>(&'a self, hostname: &'a str, port: u16) -> ProviderDnsFuture<'a>;
}

struct SystemDnsResolver;

impl ProviderDnsResolver for SystemDnsResolver {
    fn resolve<'a>(&'a self, hostname: &'a str, port: u16) -> ProviderDnsFuture<'a> {
        Box::pin(async move {
            tokio::net::lookup_host((hostname, port))
                .await
                .map(|addresses| addresses.collect())
                .map_err(|_| ProviderTransportError::Unavailable)
        })
    }
}

pub(super) type ProviderTcpConnectFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BoxedIo, ProviderTransportError>> + Send + 'a>>;

pub(super) trait ProviderTcpConnector: Send + Sync {
    fn connect<'a>(&'a self, address: SocketAddr) -> ProviderTcpConnectFuture<'a>;
}

pub(super) struct TokioTcpConnector;

impl ProviderTcpConnector for TokioTcpConnector {
    fn connect<'a>(&'a self, address: SocketAddr) -> ProviderTcpConnectFuture<'a> {
        Box::pin(async move {
            let stream = TcpStream::connect(address)
                .await
                .map_err(|_| ProviderTransportError::Unavailable)?;
            let _ = stream.set_nodelay(true);
            Ok(Box::new(stream) as BoxedIo)
        })
    }
}

pub(super) type ProviderTlsConnectFuture =
    Pin<Box<dyn Future<Output = Result<BoxedIo, ProviderTransportError>> + Send + 'static>>;

pub(super) trait ProviderTlsConnector: Send + Sync {
    fn connect(&self, stream: BoxedIo) -> ProviderTlsConnectFuture;
}

pub(super) struct RustlsTlsConnector {
    connector: tokio_rustls::TlsConnector,
    server_name: ServerName<'static>,
}

impl RustlsTlsConnector {
    pub(super) fn from_config(config: Arc<ClientConfig>) -> Result<Self, ProviderTransportError> {
        let server_name = ServerName::try_from(PROVIDER_HOST.to_owned())
            .map_err(|_| ProviderTransportError::Unavailable)?;
        Ok(Self {
            connector: tokio_rustls::TlsConnector::from(config),
            server_name,
        })
    }

    fn system() -> Result<Self, ProviderTransportError> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self::from_config(Arc::new(config))
    }
}

impl ProviderTlsConnector for RustlsTlsConnector {
    fn connect(&self, stream: BoxedIo) -> ProviderTlsConnectFuture {
        let connector = self.connector.clone();
        let server_name = self.server_name.clone();
        Box::pin(async move {
            connector
                .connect(server_name, stream)
                .await
                .map(|stream| Box::new(stream) as BoxedIo)
                .map_err(|_| ProviderTransportError::Unavailable)
        })
    }
}

pub(crate) struct NativeProviderTransport {
    resolver: Arc<dyn ProviderDnsResolver>,
    tcp: Arc<dyn ProviderTcpConnector>,
    tls: Arc<dyn ProviderTlsConnector>,
    timeout: Duration,
}

impl NativeProviderTransport {
    pub(crate) fn new() -> Result<Self, ProviderTransportError> {
        Ok(Self::with_dependencies(
            Arc::new(SystemDnsResolver),
            Arc::new(TokioTcpConnector),
            Arc::new(RustlsTlsConnector::system()?),
            PROVIDER_TIMEOUT,
        ))
    }

    pub(super) fn with_dependencies(
        resolver: Arc<dyn ProviderDnsResolver>,
        tcp: Arc<dyn ProviderTcpConnector>,
        tls: Arc<dyn ProviderTlsConnector>,
        timeout: Duration,
    ) -> Self {
        Self {
            resolver,
            tcp,
            tls,
            timeout,
        }
    }

    async fn execute_with_deadline(
        &self,
        request: ProviderHttpRequest,
    ) -> Result<ProviderHttpResponse, ProviderTransportError> {
        tokio::time::timeout(self.timeout, self.execute_inner(request))
            .await
            .map_err(|_| ProviderTransportError::Unavailable)?
    }

    async fn execute_inner(
        &self,
        request: ProviderHttpRequest,
    ) -> Result<ProviderHttpResponse, ProviderTransportError> {
        if !request.is_exact() {
            return Err(ProviderTransportError::Unavailable);
        }
        let addresses = self.resolver.resolve(PROVIDER_HOST, PROVIDER_PORT).await?;
        let mut connected = None;
        for address in addresses {
            if let Ok(stream) = self.tcp.connect(address).await {
                connected = Some(stream);
                break;
            }
        }
        let stream = connected.ok_or(ProviderTransportError::Unavailable)?;
        let stream = self.tls.connect(stream).await?;

        let mut builder = http1::Builder::new();
        builder
            .max_headers(MAX_RESPONSE_HEADER_FIELDS)
            .max_buf_size(MAX_RESPONSE_HEADER_BYTES);
        let (mut sender, connection) = builder
            .handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
            .await
            .map_err(map_hyper_error)?;
        let driver = AbortOnDrop(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let outgoing = Request::builder()
            .method(Method::GET)
            .uri(request.path_and_query())
            .header(HOST, PROVIDER_HOST)
            .header(ACCEPT, "application/json")
            .header(CONNECTION, "close")
            .body(Empty::<Bytes>::new())
            .map_err(|_| ProviderTransportError::Unavailable)?;
        let response = sender
            .send_request(outgoing)
            .await
            .map_err(map_hyper_error)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
            .collect();
        let mut incoming = response.into_body();
        let mut body = Vec::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(map_hyper_error)?;
            if let Some(data) = frame.data_ref() {
                let next_len = body
                    .len()
                    .checked_add(data.len())
                    .ok_or(ProviderTransportError::ResponseTooLarge)?;
                if next_len > MAX_RESPONSE_BODY_BYTES {
                    return Err(ProviderTransportError::ResponseTooLarge);
                }
                body.extend_from_slice(data);
            }
        }
        drop(driver);
        ProviderHttpResponse::try_new(status, headers, body)
    }
}

impl ProviderTransport for NativeProviderTransport {
    fn execute<'a>(&'a self, request: ProviderHttpRequest) -> ProviderTransportFuture<'a> {
        Box::pin(self.execute_with_deadline(request))
    }
}

fn map_hyper_error(_error: hyper::Error) -> ProviderTransportError {
    ProviderTransportError::Unavailable
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
