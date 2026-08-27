use std::{
    future::Future,
    io,
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
    sync::{
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{
    client::conn::http1,
    header::{HeaderName, HeaderValue, CONNECTION, HOST},
    Method, Request,
};
use hyper_util::rt::TokioIo;
use native_tls::{Certificate, Protocol};
use tokio::{net::TcpStream, sync::oneshot, task::JoinHandle, time::Instant};
use tokio_native_tls::TlsConnector;
use tokio_util::sync::CancellationToken;

use super::address_policy::{validate_resolved_addresses, AddressPolicyError};

const DNS_WORKERS: usize = 4;
const DNS_QUEUE_CAPACITY: usize = 16;
const RESPONSE_HEADER_FIELDS: usize = 64;
const RESPONSE_HEADER_BYTES: usize = 32 * 1024;

type Lookup = dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync + 'static;

struct DnsJob {
    host: String,
    port: u16,
    delivery: oneshot::Sender<Result<Vec<SocketAddr>, DnsResolveError>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DnsResolveError {
    Busy,
    Cancelled,
    Timeout,
    Failed,
    Empty,
    Denied,
}

#[derive(Clone)]
pub(super) struct BoundedDnsResolver {
    sender: SyncSender<DnsJob>,
}

impl BoundedDnsResolver {
    pub(super) fn new() -> io::Result<Self> {
        Self::build(DNS_WORKERS, DNS_QUEUE_CAPACITY, |host, port| {
            (host, port).to_socket_addrs().map(Iterator::collect)
        })
    }

    fn build(
        worker_count: usize,
        queue_capacity: usize,
        lookup: impl Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<DnsJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let lookup: Arc<Lookup> = Arc::new(lookup);
        for index in 0..worker_count {
            let receiver = receiver.clone();
            let lookup = lookup.clone();
            thread::Builder::new()
                .name(format!("plugin-dns-{index}"))
                .spawn(move || loop {
                    let job = {
                        let receiver = receiver.lock().unwrap_or_else(|error| error.into_inner());
                        receiver.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    let result = lookup(&job.host, job.port)
                        .map_err(|_| DnsResolveError::Failed)
                        .and_then(|addresses| validate_dns_answers(addresses, job.port));
                    let _ = job.delivery.send(result);
                })?;
        }
        Ok(Self { sender })
    }

    #[cfg(test)]
    fn with_lookup(
        worker_count: usize,
        queue_capacity: usize,
        lookup: impl Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync + 'static,
    ) -> Self {
        Self::build(worker_count, queue_capacity, lookup).unwrap()
    }

    pub(super) async fn resolve(
        &self,
        host: &str,
        port: u16,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SocketAddr>, DnsResolveError> {
        let (delivery, result) = oneshot::channel();
        let job = DnsJob {
            host: host.to_owned(),
            port,
            delivery,
        };
        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(DnsResolveError::Busy),
            Err(TrySendError::Disconnected(_)) => return Err(DnsResolveError::Failed),
        }
        tokio::select! {
            _ = cancellation.cancelled() => Err(DnsResolveError::Cancelled),
            _ = tokio::time::sleep_until(deadline) => Err(DnsResolveError::Timeout),
            result = result => result.unwrap_or(Err(DnsResolveError::Failed)),
        }
    }
}

fn validate_dns_answers(
    addresses: Vec<SocketAddr>,
    port: u16,
) -> Result<Vec<SocketAddr>, DnsResolveError> {
    let addresses =
        validate_resolved_addresses(addresses.into_iter().map(|address| address.ip()).collect())
            .map_err(|error| match error {
                AddressPolicyError::Empty => DnsResolveError::Empty,
                AddressPolicyError::Denied => DnsResolveError::Denied,
            })?;
    Ok(addresses
        .into_iter()
        .map(|address| SocketAddr::new(address, port))
        .collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeHttpsRequest {
    pub(super) hostname: String,
    pub(super) addresses: Vec<SocketAddr>,
    pub(super) method: Method,
    pub(super) path_and_query: String,
    pub(super) headers: Vec<(String, Vec<u8>)>,
    pub(super) body: Vec<u8>,
    pub(super) max_response_body_bytes: usize,
    pub(super) deadline: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NativeHttpsResponse {
    pub(super) status: u16,
    pub(super) headers: Vec<(String, Vec<u8>)>,
    pub(super) body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeTransportError {
    InvalidRequest,
    Connect,
    Tls,
    Protocol,
    ResponseHeadersTooLarge,
    ResponseBodyTooLarge,
    Timeout,
    Cancelled,
}

pub(super) type HttpsTransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<NativeHttpsResponse, NativeTransportError>> + Send + 'a>>;

pub(super) trait HttpsTransport: Send + Sync {
    fn execute<'a>(
        &'a self,
        request: NativeHttpsRequest,
        cancellation: &'a CancellationToken,
    ) -> HttpsTransportFuture<'a>;
}

#[derive(Clone)]
pub(super) struct NativeHttpsTransport {
    tls: TlsConnector,
}

impl NativeHttpsTransport {
    pub(super) fn new() -> Result<Self, NativeTransportError> {
        Self::build(Vec::new())
    }

    fn build(extra_roots: Vec<Certificate>) -> Result<Self, NativeTransportError> {
        let mut builder = native_tls::TlsConnector::builder();
        builder.min_protocol_version(Some(Protocol::Tlsv12));
        builder.use_sni(true);
        for root in extra_roots {
            builder.add_root_certificate(root);
        }
        let tls = builder.build().map_err(|_| NativeTransportError::Tls)?;
        Ok(Self {
            tls: TlsConnector::from(tls),
        })
    }

    #[cfg(test)]
    fn with_test_root(root: &[u8]) -> Result<Self, NativeTransportError> {
        let root = Certificate::from_der(root).map_err(|_| NativeTransportError::Tls)?;
        Self::build(vec![root])
    }

    pub(super) async fn execute(
        &self,
        request: NativeHttpsRequest,
        cancellation: &CancellationToken,
    ) -> Result<NativeHttpsResponse, NativeTransportError> {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(NativeTransportError::Cancelled),
            _ = tokio::time::sleep_until(request.deadline) => Err(NativeTransportError::Timeout),
            result = self.execute_inner(request) => result,
        }
    }

    async fn execute_inner(
        &self,
        request: NativeHttpsRequest,
    ) -> Result<NativeHttpsResponse, NativeTransportError> {
        if request.addresses.is_empty()
            || request.hostname.is_empty()
            || !request.path_and_query.starts_with('/')
        {
            return Err(NativeTransportError::InvalidRequest);
        }
        let mut connected = None;
        for address in &request.addresses {
            if let Ok(stream) = TcpStream::connect(address).await {
                let _ = stream.set_nodelay(true);
                connected = Some(stream);
                break;
            }
        }
        let stream = connected.ok_or(NativeTransportError::Connect)?;
        let stream = self
            .tls
            .connect(&request.hostname, stream)
            .await
            .map_err(|_| NativeTransportError::Tls)?;

        let mut builder = http1::Builder::new();
        builder
            .max_headers(RESPONSE_HEADER_FIELDS)
            .max_buf_size(RESPONSE_HEADER_BYTES);
        let (mut sender, connection) = builder
            .handshake::<_, Full<Bytes>>(TokioIo::new(stream))
            .await
            .map_err(map_hyper_error)?;
        let driver = AbortOnDrop(tokio::spawn(async move {
            let _ = connection.await;
        }));

        let mut outgoing = Request::builder()
            .method(request.method)
            .uri(&request.path_and_query)
            .body(Full::new(Bytes::from(request.body)))
            .map_err(|_| NativeTransportError::InvalidRequest)?;
        outgoing.headers_mut().insert(
            HOST,
            HeaderValue::from_str(&request.hostname)
                .map_err(|_| NativeTransportError::InvalidRequest)?,
        );
        outgoing
            .headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("close"));
        for (name, value) in request.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| NativeTransportError::InvalidRequest)?;
            let value = HeaderValue::from_bytes(&value)
                .map_err(|_| NativeTransportError::InvalidRequest)?;
            outgoing.headers_mut().append(name, value);
        }

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
                let Some(next_len) = body.len().checked_add(data.len()) else {
                    return Err(NativeTransportError::ResponseBodyTooLarge);
                };
                if next_len > request.max_response_body_bytes {
                    return Err(NativeTransportError::ResponseBodyTooLarge);
                }
                body.extend_from_slice(data);
            }
        }
        drop(driver);
        Ok(NativeHttpsResponse {
            status,
            headers,
            body,
        })
    }
}

impl HttpsTransport for NativeHttpsTransport {
    fn execute<'a>(
        &'a self,
        request: NativeHttpsRequest,
        cancellation: &'a CancellationToken,
    ) -> HttpsTransportFuture<'a> {
        Box::pin(NativeHttpsTransport::execute(self, request, cancellation))
    }
}

#[cfg(test)]
pub(super) struct DeterministicHttpsTransport {
    outcomes: Mutex<std::collections::VecDeque<Result<NativeHttpsResponse, NativeTransportError>>>,
    requests: Mutex<Vec<NativeHttpsRequest>>,
}

#[cfg(test)]
impl DeterministicHttpsTransport {
    pub(super) fn new(outcomes: Vec<Result<NativeHttpsResponse, NativeTransportError>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn take_requests(&self) -> Vec<NativeHttpsRequest> {
        std::mem::take(
            &mut *self
                .requests
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }
}

#[cfg(test)]
impl HttpsTransport for DeterministicHttpsTransport {
    fn execute<'a>(
        &'a self,
        request: NativeHttpsRequest,
        cancellation: &'a CancellationToken,
    ) -> HttpsTransportFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(NativeTransportError::Cancelled);
            }
            self.requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(request);
            self.outcomes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pop_front()
                .unwrap_or(Err(NativeTransportError::Protocol))
        })
    }
}

fn map_hyper_error(error: hyper::Error) -> NativeTransportError {
    if error.is_parse_too_large() {
        NativeTransportError::ResponseHeadersTooLarge
    } else {
        NativeTransportError::Protocol
    }
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::{poll_fn, Future},
        io,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        process::Command,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Condvar, Mutex,
        },
        task::Poll,
        time::Duration,
    };

    use hyper::Method;
    use rcgen::{
        date_time_ymd, generate_simple_self_signed, CertificateParams, CertifiedKey, KeyPair,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::{mpsc as tokio_mpsc, oneshot},
        time::Instant,
    };
    use tokio_rustls::{
        rustls::{
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
            ServerConfig,
        },
        TlsAcceptor,
    };
    use tokio_util::sync::CancellationToken;

    use super::{
        BoundedDnsResolver, DnsResolveError, NativeHttpsRequest, NativeHttpsResponse,
        NativeHttpsTransport, NativeTransportError, DNS_QUEUE_CAPACITY, DNS_WORKERS,
    };
    use super::{DeterministicHttpsTransport, HttpsTransport};

    struct ServerReply {
        delay: Duration,
        chunks: Vec<Vec<u8>>,
    }

    async fn spawn_tls_server(
        hostname: &str,
        replies: Vec<ServerReply>,
    ) -> (
        SocketAddr,
        Vec<u8>,
        tokio_mpsc::UnboundedReceiver<()>,
        tokio::task::JoinHandle<()>,
    ) {
        spawn_tls_server_with_certificate(
            generate_simple_self_signed(vec![hostname.to_owned()]).unwrap(),
            replies,
        )
        .await
    }

    async fn spawn_tls_server_with_certificate(
        certified_key: CertifiedKey<KeyPair>,
        replies: Vec<ServerReply>,
    ) -> (
        SocketAddr,
        Vec<u8>,
        tokio_mpsc::UnboundedReceiver<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let CertifiedKey { cert, signing_key } = certified_key;
        let cert_der = cert.der().to_vec();
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(cert_der.clone())], key)
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_seen, seen) = tokio_mpsc::unbounded_channel();
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
                let _ = request_seen.send(());
                tokio::time::sleep(reply.delay).await;
                for chunk in reply.chunks {
                    if stream.write_all(&chunk).await.is_err() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                let _ = stream.shutdown().await;
            }
        });
        (address, cert_der, seen, task)
    }

    fn request(address: SocketAddr, hostname: &str) -> NativeHttpsRequest {
        NativeHttpsRequest {
            hostname: hostname.to_owned(),
            addresses: vec![address],
            method: Method::GET,
            path_and_query: "/translate?q=test".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
            max_response_body_bytes: 1024 * 1024,
            deadline: Instant::now() + Duration::from_secs(3),
        }
    }

    fn public_answer(port: u16) -> Vec<SocketAddr> {
        vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), port)]
    }

    async fn resolve_after_first_poll(
        resolver: BoundedDnsResolver,
        host: String,
        cancellation: CancellationToken,
        polled: oneshot::Sender<()>,
    ) -> Result<Vec<SocketAddr>, DnsResolveError> {
        let mut resolve = Box::pin(resolver.resolve(
            &host,
            443,
            Instant::now() + Duration::from_secs(5),
            &cancellation,
        ));
        let immediate = poll_fn(|context| match resolve.as_mut().poll(context) {
            Poll::Pending => Poll::Ready(None),
            Poll::Ready(result) => Poll::Ready(Some(result)),
        })
        .await;
        let _ = polled.send(());
        match immediate {
            Some(result) => result,
            None => resolve.await,
        }
    }

    #[tokio::test]
    async fn native_https_transport_dns_resolver_returns_validated_frozen_answers() {
        drop(BoundedDnsResolver::new().unwrap());
        let resolver = BoundedDnsResolver::with_lookup(1, 1, |_host, port| Ok(public_answer(port)));
        let result = resolver
            .resolve(
                "api.example.com",
                443,
                Instant::now() + Duration::from_secs(1),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result, public_answer(443));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_https_transport_dns_cancellation_keeps_abandoned_lookup_bounded() {
        let started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let resolver = BoundedDnsResolver::with_lookup(1, 1, {
            let started = started.clone();
            let release = release.clone();
            move |_host, port| {
                started.fetch_add(1, Ordering::SeqCst);
                let (lock, ready) = &*release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                Ok(public_answer(port))
            }
        });

        let first_cancel = CancellationToken::new();
        let first = tokio::spawn({
            let resolver = resolver.clone();
            let cancel = first_cancel.clone();
            async move {
                resolver
                    .resolve(
                        "first.example.com",
                        443,
                        Instant::now() + Duration::from_secs(5),
                        &cancel,
                    )
                    .await
            }
        });
        while started.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        let second = tokio::spawn({
            let resolver = resolver.clone();
            async move {
                resolver
                    .resolve(
                        "second.example.com",
                        443,
                        Instant::now() + Duration::from_secs(5),
                        &CancellationToken::new(),
                    )
                    .await
            }
        });
        tokio::task::yield_now().await;
        first_cancel.cancel();
        assert_eq!(first.await.unwrap(), Err(DnsResolveError::Cancelled));

        assert_eq!(
            resolver
                .resolve(
                    "overflow.example.com",
                    443,
                    Instant::now() + Duration::from_secs(1),
                    &CancellationToken::new(),
                )
                .await,
            Err(DnsResolveError::Busy)
        );
        assert_eq!(started.load(Ordering::SeqCst), 1);

        let (lock, ready) = &*release;
        *lock.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(second.await.unwrap().unwrap(), public_answer(443));
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn native_https_transport_dns_production_worker_and_queue_bounds_are_fixed() {
        assert_eq!(DNS_WORKERS, 4);
        assert_eq!(DNS_QUEUE_CAPACITY, 16);

        let started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let resolver = BoundedDnsResolver::with_lookup(DNS_WORKERS, DNS_QUEUE_CAPACITY, {
            let started = started.clone();
            let release = release.clone();
            move |_host, port| {
                started.fetch_add(1, Ordering::SeqCst);
                let (lock, ready) = &*release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                Ok(public_answer(port))
            }
        });

        let mut pending = Vec::new();
        let mut cancellations = Vec::new();
        for index in 0..DNS_WORKERS {
            let cancellation = CancellationToken::new();
            let (polled, wait_until_polled) = oneshot::channel();
            pending.push(tokio::spawn(resolve_after_first_poll(
                resolver.clone(),
                format!("active-{index}.example.com"),
                cancellation.clone(),
                polled,
            )));
            cancellations.push(cancellation);
            wait_until_polled.await.unwrap();
            while started.load(Ordering::SeqCst) < index + 1 {
                tokio::task::yield_now().await;
            }
        }
        for index in 0..DNS_QUEUE_CAPACITY {
            let cancellation = CancellationToken::new();
            let (polled, wait_until_polled) = oneshot::channel();
            pending.push(tokio::spawn(resolve_after_first_poll(
                resolver.clone(),
                format!("queued-{index}.example.com"),
                cancellation.clone(),
                polled,
            )));
            cancellations.push(cancellation);
            wait_until_polled.await.unwrap();
        }

        assert_eq!(
            resolver
                .resolve(
                    "overflow.example.com",
                    443,
                    Instant::now() + Duration::from_secs(1),
                    &CancellationToken::new(),
                )
                .await,
            Err(DnsResolveError::Busy)
        );
        assert_eq!(started.load(Ordering::SeqCst), DNS_WORKERS);

        for cancellation in cancellations {
            cancellation.cancel();
        }
        for task in pending {
            assert_eq!(task.await.unwrap(), Err(DnsResolveError::Cancelled));
        }
        let (lock, ready) = &*release;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_https_transport_dns_timeout_returns_before_blocking_lookup_exits() {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let lookup_exited = Arc::new(AtomicBool::new(false));
        let resolver = BoundedDnsResolver::with_lookup(1, 1, {
            let release = release.clone();
            let lookup_exited = lookup_exited.clone();
            move |_host, port| {
                let (lock, ready) = &*release;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                lookup_exited.store(true, Ordering::SeqCst);
                Ok::<_, io::Error>(public_answer(port))
            }
        });

        let started_at = Instant::now();
        assert_eq!(
            resolver
                .resolve(
                    "timeout.example.com",
                    443,
                    Instant::now() + Duration::from_millis(30),
                    &CancellationToken::new(),
                )
                .await,
            Err(DnsResolveError::Timeout)
        );
        assert!(started_at.elapsed() < Duration::from_millis(500));
        assert!(!lookup_exited.load(Ordering::SeqCst));

        let (lock, ready) = &*release;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }

    #[tokio::test]
    async fn native_https_transport_pins_address_and_validates_original_tls_hostname() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\nX-Test: one\r\nX-Test: two\r\n\r\nok".to_vec();
        let (address, root, mut seen, server) = spawn_tls_server(
            "api.example.com",
            vec![ServerReply {
                delay: Duration::ZERO,
                chunks: vec![response],
            }],
        )
        .await;
        let transport = NativeHttpsTransport::with_test_root(&root).unwrap();
        let result = transport
            .execute(
                request(address, "api.example.com"),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.status, 200);
        assert_eq!(result.body, b"ok");
        assert_eq!(
            result
                .headers
                .iter()
                .filter(|(name, _)| name == "x-test")
                .map(|(_, value)| value.as_slice())
                .collect::<Vec<_>>(),
            vec![b"one".as_slice(), b"two".as_slice()]
        );
        seen.recv().await.unwrap();
        server.await.unwrap();
    }

    #[test]
    fn native_https_transport_ignores_proxy_environment() {
        const CHILD: &str = "UIPILOT_NATIVE_HTTPS_PROXY_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let (address, root, _, server) = spawn_tls_server(
                        "api.example.com",
                        vec![ServerReply {
                            delay: Duration::ZERO,
                            chunks: vec![b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()],
                        }],
                    )
                    .await;
                    let response = NativeHttpsTransport::with_test_root(&root)
                        .unwrap()
                        .execute(
                            request(address, "api.example.com"),
                            &CancellationToken::new(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status, 204);
                    server.await.unwrap();
                });
            return;
        }

        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("public_plugins::network::transport::tests::native_https_transport_ignores_proxy_environment")
            .arg("--nocapture")
            .env(CHILD, "1")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    async fn native_https_transport_never_downgrades_to_plain_http() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut client_hello = [0_u8; 512];
            let _ = stream.read(&mut client_hello).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        });

        assert_eq!(
            NativeHttpsTransport::new()
                .unwrap()
                .execute(
                    request(address, "api.example.com"),
                    &CancellationToken::new(),
                )
                .await,
            Err(NativeTransportError::Tls)
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn native_https_transport_rejects_untrusted_and_wrong_hostname_tls() {
        let reply = || ServerReply {
            delay: Duration::ZERO,
            chunks: vec![b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_vec()],
        };
        let (untrusted_address, _, _, untrusted_server) =
            spawn_tls_server("api.example.com", vec![reply()]).await;
        assert_eq!(
            NativeHttpsTransport::new()
                .unwrap()
                .execute(
                    request(untrusted_address, "api.example.com"),
                    &CancellationToken::new(),
                )
                .await,
            Err(NativeTransportError::Tls)
        );
        untrusted_server.abort();

        let (wrong_address, root, _, wrong_server) =
            spawn_tls_server("api.example.com", vec![reply()]).await;
        assert_eq!(
            NativeHttpsTransport::with_test_root(&root)
                .unwrap()
                .execute(
                    request(wrong_address, "other.example.com"),
                    &CancellationToken::new(),
                )
                .await,
            Err(NativeTransportError::Tls)
        );
        wrong_server.abort();
    }

    #[tokio::test]
    async fn native_https_transport_rejects_expired_certificate() {
        let signing_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec!["api.example.com".to_owned()]).unwrap();
        params.not_before = date_time_ymd(2010, 1, 1);
        params.not_after = date_time_ymd(2011, 1, 1);
        let certified_key = CertifiedKey {
            cert: params.self_signed(&signing_key).unwrap(),
            signing_key,
        };
        let (address, root, _, server) = spawn_tls_server_with_certificate(
            certified_key,
            vec![ServerReply {
                delay: Duration::ZERO,
                chunks: Vec::new(),
            }],
        )
        .await;
        assert_eq!(
            NativeHttpsTransport::with_test_root(&root)
                .unwrap()
                .execute(
                    request(address, "api.example.com"),
                    &CancellationToken::new(),
                )
                .await,
            Err(NativeTransportError::Tls)
        );
        server.abort();
    }

    #[tokio::test]
    async fn native_https_transport_enforces_parser_header_count_and_byte_limits() {
        let mut too_many = String::from("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n");
        for index in 0..65 {
            too_many.push_str(&format!("X-{index}: value\r\n"));
        }
        too_many.push_str("\r\n");
        let oversized = format!(
            "HTTP/1.1 200 OK\r\nX-Large: {}\r\nContent-Length: 0\r\n\r\n",
            "a".repeat(33 * 1024)
        );
        for raw in [too_many.into_bytes(), oversized.into_bytes()] {
            let (address, root, _, server) = spawn_tls_server(
                "api.example.com",
                vec![ServerReply {
                    delay: Duration::ZERO,
                    chunks: vec![raw],
                }],
            )
            .await;
            assert_eq!(
                NativeHttpsTransport::with_test_root(&root)
                    .unwrap()
                    .execute(
                        request(address, "api.example.com"),
                        &CancellationToken::new(),
                    )
                    .await,
                Err(NativeTransportError::ResponseHeadersTooLarge)
            );
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn native_https_transport_stops_progressive_header_and_body_overflow() {
        let progressive = vec![
            b"HTTP/1.1 200 OK\r\nX-Large: ".to_vec(),
            vec![b'a'; 12 * 1024],
            vec![b'a'; 12 * 1024],
            vec![b'a'; 12 * 1024],
            b"\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ];
        let (header_address, header_root, _, header_server) = spawn_tls_server(
            "api.example.com",
            vec![ServerReply {
                delay: Duration::ZERO,
                chunks: progressive,
            }],
        )
        .await;
        assert_eq!(
            NativeHttpsTransport::with_test_root(&header_root)
                .unwrap()
                .execute(
                    request(header_address, "api.example.com"),
                    &CancellationToken::new(),
                )
                .await,
            Err(NativeTransportError::ResponseHeadersTooLarge)
        );
        header_server.await.unwrap();

        let body = vec![b'x'; 1025];
        let mut raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        raw.extend(body);
        let (body_address, body_root, _, body_server) = spawn_tls_server(
            "api.example.com",
            vec![ServerReply {
                delay: Duration::ZERO,
                chunks: vec![raw],
            }],
        )
        .await;
        let mut body_request = request(body_address, "api.example.com");
        body_request.max_response_body_bytes = 1024;
        assert_eq!(
            NativeHttpsTransport::with_test_root(&body_root)
                .unwrap()
                .execute(body_request, &CancellationToken::new())
                .await,
            Err(NativeTransportError::ResponseBodyTooLarge)
        );
        body_server.await.unwrap();
    }

    #[tokio::test]
    async fn native_https_transport_cancels_promptly_and_never_reuses_connections() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n".to_vec();
        let (address, root, mut seen, server) = spawn_tls_server(
            "api.example.com",
            vec![
                ServerReply {
                    delay: Duration::ZERO,
                    chunks: vec![ok.clone()],
                },
                ServerReply {
                    delay: Duration::ZERO,
                    chunks: vec![ok],
                },
                ServerReply {
                    delay: Duration::from_secs(5),
                    chunks: Vec::new(),
                },
            ],
        )
        .await;
        let transport = NativeHttpsTransport::with_test_root(&root).unwrap();
        for _ in 0..2 {
            transport
                .execute(
                    request(address, "api.example.com"),
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            seen.recv().await.unwrap();
        }

        let cancellation = CancellationToken::new();
        let pending = tokio::spawn({
            let transport = transport.clone();
            let cancellation = cancellation.clone();
            async move {
                transport
                    .execute(request(address, "api.example.com"), &cancellation)
                    .await
            }
        });
        seen.recv().await.unwrap();
        let cancelled_at = Instant::now();
        cancellation.cancel();
        assert_eq!(pending.await.unwrap(), Err(NativeTransportError::Cancelled));
        assert!(cancelled_at.elapsed() < Duration::from_millis(500));
        server.abort();
    }

    #[tokio::test]
    async fn native_https_transport_deterministic_adapter_records_and_replays() {
        let expected = NativeHttpsResponse {
            status: 503,
            headers: vec![("content-type".into(), b"text/plain".to_vec())],
            body: b"later".to_vec(),
        };
        let transport = DeterministicHttpsTransport::new(vec![Ok(expected.clone())]);
        let input = request(SocketAddr::from(([8, 8, 8, 8], 443)), "api.example.com");
        let result =
            HttpsTransport::execute(&transport, input.clone(), &CancellationToken::new()).await;
        assert_eq!(result, Ok(expected));
        assert_eq!(transport.take_requests(), vec![input]);
    }
}
