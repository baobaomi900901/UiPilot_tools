use std::{
    collections::{BTreeMap, VecDeque},
    future::pending,
    io,
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    task::{Context, Poll},
    time::{Duration, Instant as StdInstant},
};

use native_tls::{Certificate, Protocol};
use rcgen::{generate_simple_self_signed, CertifiedKey};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpListener,
    sync::mpsc,
};
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig,
    },
    TlsAcceptor,
};

use super::{
    provider::{ExchangeRateProvider, FrankfurterProvider},
    transport::{
        BoxedIo, NativeProviderTransport, NativeTlsConnector, ProviderDnsFuture,
        ProviderDnsResolver, ProviderHttpRequest, ProviderHttpResponse, ProviderTcpConnectFuture,
        ProviderTcpConnector, ProviderTlsConnectFuture, ProviderTlsConnector, ProviderTransport,
        ProviderTransportError, ProviderTransportFuture, TokioTcpConnector, PROVIDER_TIMEOUT,
    },
    CurrencyCode, ExchangeRateError, ExchangeRateRequest,
};

struct RecordingTransport {
    requests: Mutex<Vec<ProviderHttpRequest>>,
    outcomes: Mutex<VecDeque<Result<ProviderHttpResponse, ProviderTransportError>>>,
}

#[derive(Clone)]
struct FixedResolver {
    address: SocketAddr,
    calls: Arc<Mutex<Vec<(String, u16)>>>,
}

impl FixedResolver {
    fn new(address: SocketAddr) -> Self {
        Self {
            address,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<(String, u16)> {
        self.calls.lock().unwrap().clone()
    }
}

impl ProviderDnsResolver for FixedResolver {
    fn resolve<'a>(&'a self, hostname: &'a str, port: u16) -> ProviderDnsFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((hostname.to_owned(), port));
            Ok(vec![self.address])
        })
    }
}

struct PendingResolver;

impl ProviderDnsResolver for PendingResolver {
    fn resolve<'a>(&'a self, _hostname: &'a str, _port: u16) -> ProviderDnsFuture<'a> {
        Box::pin(pending())
    }
}

struct PendingConnector;

impl ProviderTcpConnector for PendingConnector {
    fn connect<'a>(&'a self, _address: SocketAddr) -> ProviderTcpConnectFuture<'a> {
        Box::pin(pending())
    }
}

struct PendingTlsConnector;

impl ProviderTlsConnector for PendingTlsConnector {
    fn connect(&self, _hostname: String, _stream: BoxedIo) -> ProviderTlsConnectFuture {
        Box::pin(pending())
    }
}

struct BlockingPollTlsConnector {
    entered: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl ProviderTlsConnector for BlockingPollTlsConnector {
    fn connect(&self, _hostname: String, _stream: BoxedIo) -> ProviderTlsConnectFuture {
        Box::pin(BlockingPollTlsFuture {
            entered: self.entered.clone(),
            finished: self.finished.clone(),
            release: self.release.clone(),
        })
    }
}

struct BlockingPollTlsFuture {
    entered: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl std::future::Future for BlockingPollTlsFuture {
    type Output = Result<BoxedIo, ProviderTransportError>;

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        self.entered.store(true, Ordering::SeqCst);
        let (lock, ready) = &*self.release;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = ready.wait(released).unwrap();
        }
        self.finished.store(true, Ordering::SeqCst);
        Poll::Ready(Err(ProviderTransportError::Unavailable))
    }
}

struct PassthroughTlsConnector;

impl ProviderTlsConnector for PassthroughTlsConnector {
    fn connect(&self, _hostname: String, stream: BoxedIo) -> ProviderTlsConnectFuture {
        Box::pin(async move { Ok(stream) })
    }
}

struct PendingIo;

impl AsyncRead for PendingIo {
    fn poll_read(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for PendingIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Pending
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Pending
    }
}

struct PendingIoConnector;

impl ProviderTcpConnector for PendingIoConnector {
    fn connect<'a>(&'a self, _address: SocketAddr) -> ProviderTcpConnectFuture<'a> {
        Box::pin(async { Ok(Box::new(PendingIo) as BoxedIo) })
    }
}

struct ServerReply {
    chunks: Vec<(Duration, Vec<u8>)>,
}

async fn spawn_tls_server(
    certificate_hostname: &str,
    replies: Vec<ServerReply>,
) -> (
    SocketAddr,
    Vec<u8>,
    mpsc::UnboundedReceiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec![certificate_hostname.to_owned()]).unwrap();
    let cert_der = cert.der().to_vec();
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert_der.clone())], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        for reply in replies {
            let (stream, _) = listener.accept().await.unwrap();
            let Ok(mut stream) = acceptor.accept(stream).await else {
                continue;
            };
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let _ = request_tx.send(request);
            for (delay, chunk) in reply.chunks {
                tokio::time::sleep(delay).await;
                if stream.write_all(&chunk).await.is_err() {
                    break;
                }
            }
            let _ = stream.shutdown().await;
        }
    });
    (address, cert_der, request_rx, task)
}

fn tls_with_root(root: &[u8]) -> Arc<dyn ProviderTlsConnector> {
    let root = Certificate::from_der(root).unwrap();
    let mut builder = native_tls::TlsConnector::builder();
    builder.min_protocol_version(Some(Protocol::Tlsv12));
    builder.use_sni(true);
    builder.add_root_certificate(root);
    let connector = tokio_native_tls::TlsConnector::from(builder.build().unwrap());
    Arc::new(NativeTlsConnector::from_connector(connector))
}

fn native_provider(
    resolver: Arc<dyn ProviderDnsResolver>,
    connector: Arc<dyn ProviderTcpConnector>,
    tls: Arc<dyn ProviderTlsConnector>,
    timeout: Duration,
) -> FrankfurterProvider {
    FrankfurterProvider::with_transport(Arc::new(NativeProviderTransport::with_dependencies(
        resolver, connector, tls, timeout,
    )))
}

async fn execute_native_reply(
    raw: Vec<u8>,
) -> Result<ProviderHttpResponse, ProviderTransportError> {
    let (address, root, _, server) = spawn_tls_server(
        "api.frankfurter.dev",
        vec![ServerReply {
            chunks: vec![(Duration::ZERO, raw)],
        }],
    )
    .await;
    let transport = NativeProviderTransport::with_dependencies(
        Arc::new(FixedResolver::new(address)),
        Arc::new(TokioTcpConnector),
        tls_with_root(&root),
        Duration::from_secs(2),
    );
    let result = transport
        .execute(ProviderHttpRequest::for_exchange(&request()).unwrap())
        .await;
    server.abort();
    let _ = server.await;
    result
}

async fn assert_deadline(provider: &FrankfurterProvider) {
    let started = StdInstant::now();
    assert_eq!(
        provider.fetch_rate(&request()).await,
        Err(ExchangeRateError::Unavailable)
    );
    assert!(started.elapsed() < Duration::from_millis(500));
}

impl RecordingTransport {
    fn new(outcomes: Vec<Result<ProviderHttpResponse, ProviderTransportError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            outcomes: Mutex::new(outcomes.into()),
        }
    }

    fn take_requests(&self) -> Vec<ProviderHttpRequest> {
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}

impl ProviderTransport for RecordingTransport {
    fn execute<'a>(&'a self, request: ProviderHttpRequest) -> ProviderTransportFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(ProviderTransportError::Unavailable))
        })
    }
}

fn request() -> ExchangeRateRequest {
    ExchangeRateRequest {
        base: CurrencyCode::new("USD").unwrap(),
        quote: CurrencyCode::new("CNY").unwrap(),
    }
}

fn response(status: u16, body: &[u8]) -> ProviderHttpResponse {
    ProviderHttpResponse::try_new(status, Vec::new(), body.to_vec()).unwrap()
}

#[tokio::test]
async fn provider_constructs_only_the_exact_fixed_origin_request() {
    let transport = Arc::new(RecordingTransport::new(vec![Ok(response(
        200,
        br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.13245}"#,
    ))]));
    let provider = FrankfurterProvider::with_transport(transport.clone());

    provider.fetch_rate(&request()).await.unwrap();

    let requests = transport.take_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method(), "GET");
    assert_eq!(requests[0].path_and_query(), "/v2/rate/USD/CNY");
    assert_eq!(
        requests[0].headers(),
        &[
            ("host".to_owned(), b"api.frankfurter.dev".to_vec()),
            ("accept".to_owned(), b"application/json".to_vec()),
        ]
    );
}

#[tokio::test]
async fn noncanonical_or_path_like_codes_never_reach_the_transport() {
    for (base, quote) in [
        ("usd", "CNY"),
        ("US/", "CNY"),
        ("USD", "CN?"),
        ("USD%2FCNY", "EUR"),
    ] {
        let transport = Arc::new(RecordingTransport::new(Vec::new()));
        let provider = FrankfurterProvider::with_transport(transport.clone());
        let invalid = ExchangeRateRequest {
            base: CurrencyCode(base.to_owned()),
            quote: CurrencyCode(quote.to_owned()),
        };

        assert_eq!(
            provider.fetch_rate(&invalid).await,
            Err(ExchangeRateError::Unavailable),
            "base={base:?}, quote={quote:?}"
        );
        assert!(transport.take_requests().is_empty());
    }
}

#[test]
fn transport_response_accepts_exactly_64_kib_and_rejects_the_sentinel_byte() {
    assert!(ProviderHttpResponse::try_new(200, Vec::new(), vec![b'x'; 64 * 1024]).is_ok());
    assert_eq!(
        ProviderHttpResponse::try_new(200, Vec::new(), vec![b'x'; 64 * 1024 + 1]),
        Err(ProviderTransportError::ResponseTooLarge)
    );
}

#[test]
fn transport_request_and_response_debug_are_structurally_redacted() {
    let outgoing = ProviderHttpRequest::for_exchange(&request()).unwrap();
    assert_eq!(
        format!("{outgoing:?}"),
        "ProviderHttpRequest { redacted: true }"
    );

    let incoming = ProviderHttpResponse::try_new(
        503,
        vec![(
            "x-private-sentinel".to_owned(),
            b"header-secret-sentinel".to_vec(),
        )],
        b"body-secret-sentinel".to_vec(),
    )
    .unwrap();
    assert_eq!(
        format!("{incoming:?}"),
        "ProviderHttpResponse { status: 503, redacted: true }"
    );
    assert_eq!(
        format!("{:?}", ProviderTransportError::ResponseTooLarge),
        "ResponseTooLarge"
    );
}

#[tokio::test]
async fn provider_maps_only_422_to_unsupported_and_never_exposes_error_bodies() {
    for (status, expected) in [
        (301, ExchangeRateError::Unavailable),
        (404, ExchangeRateError::Unavailable),
        (422, ExchangeRateError::UnsupportedCode),
        (429, ExchangeRateError::Unavailable),
        (500, ExchangeRateError::Unavailable),
        (503, ExchangeRateError::Unavailable),
    ] {
        let secret = format!("provider secret for {status}");
        let transport = Arc::new(RecordingTransport::new(vec![Ok(response(
            status,
            secret.as_bytes(),
        ))]));
        let provider = FrankfurterProvider::with_transport(transport);

        let error = provider.fetch_rate(&request()).await.unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?}").contains(&secret));
    }
}

#[tokio::test]
async fn native_https_uses_fixed_dns_tls_http1_and_exact_wire_request() {
    let body = br#"{"date":"2026-09-03","base":"USD","quote":"CNY","rate":7.13245}"#;
    let mut raw_response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    raw_response.extend_from_slice(body);
    let (address, root, mut seen, server) = spawn_tls_server(
        "api.frankfurter.dev",
        vec![ServerReply {
            chunks: vec![(Duration::ZERO, raw_response)],
        }],
    )
    .await;
    let resolver = FixedResolver::new(address);
    let provider = native_provider(
        Arc::new(resolver.clone()),
        Arc::new(TokioTcpConnector),
        tls_with_root(&root),
        Duration::from_secs(2),
    );

    provider.fetch_rate(&request()).await.unwrap();

    assert_eq!(
        resolver.calls(),
        vec![("api.frankfurter.dev".to_owned(), 443)]
    );
    let request = String::from_utf8(seen.recv().await.unwrap()).unwrap();
    let mut lines = request.split("\r\n");
    assert_eq!(lines.next(), Some("GET /v2/rate/USD/CNY HTTP/1.1"));
    let wire_headers = lines
        .take_while(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(": ").unwrap();
            (name.to_ascii_lowercase(), value.to_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(wire_headers.len(), 3);
    assert_eq!(
        wire_headers.into_iter().collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("accept".to_owned(), "application/json".to_owned()),
            ("connection".to_owned(), "close".to_owned()),
            ("host".to_owned(), "api.frankfurter.dev".to_owned()),
        ])
    );
    server.await.unwrap();
}

#[tokio::test]
async fn native_https_rejects_wrong_hostname_and_untrusted_certificates() {
    let empty = || ServerReply { chunks: Vec::new() };
    let (wrong_address, wrong_root, _, wrong_server) =
        spawn_tls_server("other.example.com", vec![empty()]).await;
    let wrong_provider = native_provider(
        Arc::new(FixedResolver::new(wrong_address)),
        Arc::new(TokioTcpConnector),
        tls_with_root(&wrong_root),
        Duration::from_secs(2),
    );
    assert_eq!(
        wrong_provider.fetch_rate(&request()).await,
        Err(ExchangeRateError::Unavailable)
    );
    wrong_server.abort();

    let (untrusted_address, _, _, untrusted_server) =
        spawn_tls_server("api.frankfurter.dev", vec![empty()]).await;
    let untrusted_tls = native_tls::TlsConnector::builder().build().unwrap();
    let untrusted_provider = native_provider(
        Arc::new(FixedResolver::new(untrusted_address)),
        Arc::new(TokioTcpConnector),
        Arc::new(NativeTlsConnector::from_connector(
            tokio_native_tls::TlsConnector::from(untrusted_tls),
        )),
        Duration::from_secs(2),
    );
    assert_eq!(
        untrusted_provider.fetch_rate(&request()).await,
        Err(ExchangeRateError::Unavailable)
    );
    untrusted_server.abort();
}

#[tokio::test]
async fn native_https_maps_connection_failure_to_unavailable() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let provider = native_provider(
        Arc::new(FixedResolver::new(address)),
        Arc::new(TokioTcpConnector),
        Arc::new(PendingTlsConnector),
        Duration::from_secs(2),
    );

    assert_eq!(
        provider.fetch_rate(&request()).await,
        Err(ExchangeRateError::Unavailable)
    );
}

#[tokio::test]
async fn one_deadline_covers_dns_connect_tls_and_request_send() {
    let dummy = SocketAddr::from(([127, 0, 0, 1], 9));
    let cases = [
        native_provider(
            Arc::new(PendingResolver),
            Arc::new(PendingConnector),
            Arc::new(PendingTlsConnector),
            Duration::from_millis(30),
        ),
        native_provider(
            Arc::new(FixedResolver::new(dummy)),
            Arc::new(PendingConnector),
            Arc::new(PendingTlsConnector),
            Duration::from_millis(30),
        ),
        native_provider(
            Arc::new(FixedResolver::new(dummy)),
            Arc::new(PendingIoConnector),
            Arc::new(PendingTlsConnector),
            Duration::from_millis(30),
        ),
        native_provider(
            Arc::new(FixedResolver::new(dummy)),
            Arc::new(PendingIoConnector),
            Arc::new(PassthroughTlsConnector),
            Duration::from_millis(30),
        ),
    ];

    for provider in &cases {
        assert_deadline(provider).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blocking_tls_future_poll_cannot_delay_the_end_to_end_deadline() {
    let entered = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let releaser = std::thread::spawn({
        let entered = entered.clone();
        let release = release.clone();
        move || {
            while !entered.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
            std::thread::sleep(Duration::from_millis(250));
            let (lock, ready) = &*release;
            *lock.lock().unwrap() = true;
            ready.notify_all();
        }
    });
    let provider = native_provider(
        Arc::new(FixedResolver::new(SocketAddr::from(([127, 0, 0, 1], 9)))),
        Arc::new(PendingIoConnector),
        Arc::new(BlockingPollTlsConnector {
            entered,
            finished: finished.clone(),
            release,
        }),
        Duration::from_millis(30),
    );

    let started = StdInstant::now();
    let result = provider.fetch_rate(&request()).await;
    let elapsed = started.elapsed();
    releaser.join().unwrap();
    assert!(finished.load(Ordering::SeqCst));
    assert_eq!(result, Err(ExchangeRateError::Unavailable));
    assert!(
        elapsed < Duration::from_millis(150),
        "blocking TLS poll delayed the deadline for {elapsed:?}"
    );
}

#[tokio::test]
async fn the_same_deadline_covers_response_headers_and_body() {
    let (headers_address, headers_root, _, headers_server) = spawn_tls_server(
        "api.frankfurter.dev",
        vec![ServerReply {
            chunks: vec![(
                Duration::from_secs(5),
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            )],
        }],
    )
    .await;
    let headers_provider = native_provider(
        Arc::new(FixedResolver::new(headers_address)),
        Arc::new(TokioTcpConnector),
        tls_with_root(&headers_root),
        Duration::from_millis(30),
    );
    assert_deadline(&headers_provider).await;
    headers_server.abort();

    let (body_address, body_root, _, body_server) = spawn_tls_server(
        "api.frankfurter.dev",
        vec![ServerReply {
            chunks: vec![
                (
                    Duration::ZERO,
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\none"
                        .to_vec(),
                ),
                (Duration::from_secs(5), b"twothree".to_vec()),
            ],
        }],
    )
    .await;
    let body_provider = native_provider(
        Arc::new(FixedResolver::new(body_address)),
        Arc::new(TokioTcpConnector),
        tls_with_root(&body_root),
        Duration::from_millis(30),
    );
    assert_deadline(&body_provider).await;
    body_server.abort();
}

#[test]
fn production_deadline_is_exactly_two_seconds() {
    assert_eq!(PROVIDER_TIMEOUT, Duration::from_secs(2));
    FrankfurterProvider::new().unwrap();
}

#[test]
fn transport_response_bounds_header_count_and_bytes() {
    let exact_count = (0..64)
        .map(|index| (format!("x-{index}"), Vec::new()))
        .collect();
    assert!(ProviderHttpResponse::try_new(200, exact_count, Vec::new()).is_ok());
    let too_many = (0..65)
        .map(|index| (format!("x-{index}"), Vec::new()))
        .collect();
    assert_eq!(
        ProviderHttpResponse::try_new(200, too_many, Vec::new()),
        Err(ProviderTransportError::ResponseTooLarge)
    );
    assert!(ProviderHttpResponse::try_new(
        200,
        vec![("x".to_owned(), vec![b'a'; 32 * 1024 - 1])],
        Vec::new(),
    )
    .is_ok());
    assert_eq!(
        ProviderHttpResponse::try_new(
            200,
            vec![("x".to_owned(), vec![b'a'; 32 * 1024])],
            Vec::new(),
        ),
        Err(ProviderTransportError::ResponseTooLarge)
    );
}

#[tokio::test]
async fn native_http1_rejects_oversized_header_count_and_bytes_before_returning() {
    let mut too_many = String::from("HTTP/1.1 200 OK\r\n");
    for index in 0..65 {
        too_many.push_str(&format!("X-Field-{index}: value\r\n"));
    }
    too_many.push_str("Content-Length: 0\r\nConnection: close\r\n\r\n");
    let too_large = format!(
        "HTTP/1.1 200 OK\r\nX-Large: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "a".repeat(33 * 1024)
    );

    for raw in [too_many.into_bytes(), too_large.into_bytes()] {
        assert_eq!(
            execute_native_reply(raw).await,
            Err(ProviderTransportError::Unavailable)
        );
    }
}

#[tokio::test]
async fn native_http1_rejects_an_oversized_chunked_trailer_during_parse() {
    let raw = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X-Large\r\nConnection: close\r\n\r\n1\r\nx\r\n0\r\nX-Large: {}\r\n\r\n",
        "a".repeat(33 * 1024)
    )
    .into_bytes();

    assert_eq!(
        execute_native_reply(raw).await,
        Err(ProviderTransportError::Unavailable)
    );
}

#[tokio::test]
async fn native_transport_accepts_64_kib_body_and_rejects_the_next_byte() {
    for (length, expected) in [
        (64 * 1024, Ok(64 * 1024)),
        (64 * 1024 + 1, Err(ProviderTransportError::ResponseTooLarge)),
    ] {
        let body = vec![b'x'; length];
        let mut raw = format!(
            "HTTP/1.1 200 OK\r\nX-Test: bounded\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        raw.extend_from_slice(&body);
        let (address, root, _, server) = spawn_tls_server(
            "api.frankfurter.dev",
            vec![ServerReply {
                chunks: vec![(Duration::ZERO, raw)],
            }],
        )
        .await;
        let transport = NativeProviderTransport::with_dependencies(
            Arc::new(FixedResolver::new(address)),
            Arc::new(TokioTcpConnector),
            tls_with_root(&root),
            Duration::from_secs(2),
        );

        let result = transport
            .execute(ProviderHttpRequest::for_exchange(&request()).unwrap())
            .await;
        assert_eq!(
            result
                .as_ref()
                .map(|response| response.body().len())
                .map_err(|error| *error),
            expected
        );
        if let Ok(response) = result {
            assert_eq!(response.status(), 200);
            assert!(response
                .headers()
                .iter()
                .any(|(name, value)| name == "x-test" && value.as_slice() == b"bounded"));
            server.await.unwrap();
        } else {
            server.abort();
        }
    }
}

#[tokio::test]
async fn redirect_is_unavailable_and_never_causes_a_second_request() {
    let redirect = b"HTTP/1.1 302 Found\r\nLocation: https://attacker.invalid/rate\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
    let (address, root, mut seen, server) = spawn_tls_server(
        "api.frankfurter.dev",
        vec![
            ServerReply {
                chunks: vec![(Duration::ZERO, redirect)],
            },
            ServerReply {
                chunks: vec![(
                    Duration::ZERO,
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                )],
            },
        ],
    )
    .await;
    let provider = native_provider(
        Arc::new(FixedResolver::new(address)),
        Arc::new(TokioTcpConnector),
        tls_with_root(&root),
        Duration::from_secs(2),
    );

    assert_eq!(
        provider.fetch_rate(&request()).await,
        Err(ExchangeRateError::Unavailable)
    );
    seen.recv().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), seen.recv())
            .await
            .is_err()
    );
    server.abort();
}

#[tokio::test]
async fn malformed_200_body_is_unavailable_without_body_disclosure() {
    let secret = b"raw provider message: account-internal-secret";
    let transport = Arc::new(RecordingTransport::new(vec![Ok(response(200, secret))]));
    let provider = FrankfurterProvider::with_transport(transport);

    let error = provider.fetch_rate(&request()).await.unwrap_err();
    assert_eq!(error, ExchangeRateError::Unavailable);
    assert!(!format!("{error:?}").contains("account-internal-secret"));
}
