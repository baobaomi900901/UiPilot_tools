use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant as StdInstant},
};

use hyper::{header::HeaderName, Method};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;
use url::Url;

use super::{
    registry::{
        PluginNetworkCallIdentity, PluginNetworkCallTerminal, PluginNetworkContextIdentity,
        PluginNetworkRegistryError, PluginNetworkRequestRegistry, RegisteredPluginNetworkCall,
    },
    transport::{
        BoundedDnsResolver, DnsResolveError, HttpsTransport, NativeHttpsRequest,
        NativeHttpsResponse, NativeHttpsTransport, NativeTransportError,
    },
};

const MAX_URL_BYTES: usize = 2048;
const MAX_REQUEST_HEADER_FIELDS: usize = 32;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_HEADER_FIELDS: usize = 64;
const MAX_RESPONSE_HEADER_BYTES: usize = 32 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const TOTAL_DEADLINE: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "UiPilot-Plugin/0.3.3";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum PluginNetworkRequestMethod {
    Get,
    Post,
}

impl PluginNetworkRequestMethod {
    fn as_hyper(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Post => Method::POST,
        }
    }

    fn as_log(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum PluginNetworkRequestBody {
    Json { value: Value },
    Text { value: String },
    Form { value: BTreeMap<String, String> },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginNetworkRequest {
    pub(crate) url: String,
    pub(crate) method: PluginNetworkRequestMethod,
    #[serde(default)]
    pub(crate) headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub(crate) body: Option<PluginNetworkRequestBody>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginNetworkResponse {
    pub(crate) status: u16,
    pub(crate) headers: BTreeMap<String, Vec<String>>,
    pub(crate) body: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PluginNetworkErrorCode {
    InvalidNetworkRequest,
    PermissionDenied,
    NetworkTargetDenied,
    NetworkTimeout,
    NetworkFailure,
    NetworkResponseTooLarge,
    NetworkResponseInvalid,
    NetworkLimitExceeded,
    ExpiredRequest,
}

pub(crate) type RedirectAuthority =
    Arc<dyn Fn(&PluginNetworkCallIdentity, &str) -> bool + Send + Sync>;

pub(crate) struct PluginHttpsBroker {
    registry: PluginNetworkRequestRegistry,
    resolver: BoundedDnsResolver,
    transport: Arc<dyn HttpsTransport>,
}

struct ValidatedRequest {
    url: Url,
    hostname: String,
    method: PluginNetworkRequestMethod,
    plugin_headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
    content_type: Option<&'static str>,
    authorized_hosts: BTreeSet<String>,
    deadline: Instant,
    started: StdInstant,
}

pub(crate) struct PreparedPluginNetworkCall {
    request: ValidatedRequest,
    call: RegisteredPluginNetworkCall,
}

#[cfg(test)]
impl PreparedPluginNetworkCall {
    pub(crate) fn is_cancelled_for_test(&self) -> bool {
        self.call.cancellation().is_cancelled()
    }
}

pub(crate) struct PendingPluginNetworkResponse {
    response: PluginNetworkResponse,
    call: RegisteredPluginNetworkCall,
    method: PluginNetworkRequestMethod,
    hostname: String,
    started: StdInstant,
}

impl PendingPluginNetworkResponse {
    pub(super) fn identity(&self) -> &PluginNetworkCallIdentity {
        self.call.identity()
    }

    pub(super) fn response(&self) -> &PluginNetworkResponse {
        &self.response
    }

    pub(super) fn deliver(self) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        if !self.call.finish(PluginNetworkCallTerminal::Delivered) {
            log_terminal(
                self.call.identity(),
                self.method,
                &self.hostname,
                "expired",
                None,
                self.started,
            );
            return Err(PluginNetworkErrorCode::ExpiredRequest);
        }
        log_terminal(
            self.call.identity(),
            self.method,
            &self.hostname,
            "response",
            Some(self.response.status),
            self.started,
        );
        Ok(self.response)
    }
}

impl PluginHttpsBroker {
    pub(super) fn new() -> Result<Self, PluginNetworkErrorCode> {
        let resolver =
            BoundedDnsResolver::new().map_err(|_| PluginNetworkErrorCode::NetworkFailure)?;
        let transport = Arc::new(
            NativeHttpsTransport::new().map_err(|_| PluginNetworkErrorCode::NetworkFailure)?,
        );
        Ok(Self::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver,
            transport,
        ))
    }

    pub(super) fn with_dependencies<T>(
        registry: PluginNetworkRequestRegistry,
        resolver: BoundedDnsResolver,
        transport: Arc<T>,
    ) -> Self
    where
        T: HttpsTransport + 'static,
    {
        Self {
            registry,
            resolver,
            transport,
        }
    }

    pub(super) fn admit(
        &self,
        context: &PluginNetworkContextIdentity,
        authorized_hosts: &BTreeSet<String>,
        request: PluginNetworkRequest,
    ) -> Result<PreparedPluginNetworkCall, PluginNetworkErrorCode> {
        let attempt = self
            .registry
            .reserve_attempt(context)
            .map_err(map_registry_error)?;
        let request = validate_request(request, authorized_hosts)?;
        let call = self
            .registry
            .register(attempt)
            .map_err(map_registry_error)?;
        Ok(PreparedPluginNetworkCall { request, call })
    }

    pub(super) async fn execute(
        &self,
        prepared: PreparedPluginNetworkCall,
        redirect_authority: RedirectAuthority,
    ) -> Result<PendingPluginNetworkResponse, PluginNetworkErrorCode> {
        let PreparedPluginNetworkCall { request, call } = prepared;
        let method = request.method;
        let hostname = request.hostname.clone();
        let started = request.started;
        match self
            .execute_inner(&request, &call, redirect_authority)
            .await
        {
            Ok(response) => Ok(PendingPluginNetworkResponse {
                response,
                call,
                method,
                hostname,
                started,
            }),
            Err(error) => {
                let _ = call.finish(PluginNetworkCallTerminal::Failed);
                log_terminal(
                    call.identity(),
                    method,
                    &hostname,
                    error.log_category(),
                    None,
                    started,
                );
                Err(error)
            }
        }
    }

    async fn execute_inner(
        &self,
        request: &ValidatedRequest,
        call: &RegisteredPluginNetworkCall,
        redirect_authority: RedirectAuthority,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        let mut url = request.url.clone();
        let mut method = request.method;
        let mut body = request.body.clone();
        let mut content_type = request.content_type;
        let mut redirects = 0;

        loop {
            let hostname = validate_target(&url, &request.authorized_hosts)?;
            if hostname != request.hostname {
                return Err(PluginNetworkErrorCode::NetworkTargetDenied);
            }
            if redirects != 0 && !(redirect_authority)(call.identity(), &hostname) {
                return Err(PluginNetworkErrorCode::ExpiredRequest);
            }
            let addresses = self
                .resolver
                .resolve(&hostname, 443, request.deadline, call.cancellation())
                .await
                .map_err(map_dns_error)?;
            let transport_request = NativeHttpsRequest {
                hostname: hostname.clone(),
                addresses,
                method: method.as_hyper(),
                path_and_query: path_and_query(&url),
                headers: transport_headers(&request.plugin_headers, &body, content_type),
                body: body.clone(),
                max_response_body_bytes: MAX_RESPONSE_BODY_BYTES,
                deadline: request.deadline,
            };
            let response = self
                .transport
                .execute(transport_request, call.cancellation())
                .await
                .map_err(map_transport_error)?;
            let normalized_headers = validate_response_metadata(&response)?;
            if is_redirect(response.status) {
                if redirects >= MAX_REDIRECTS {
                    return Err(PluginNetworkErrorCode::NetworkTargetDenied);
                }
                let location = single_location(&normalized_headers)?;
                let next = url
                    .join(location)
                    .map_err(|_| PluginNetworkErrorCode::NetworkTargetDenied)?;
                let next_hostname = validate_target(&next, &request.authorized_hosts)?;
                if next_hostname != request.hostname {
                    return Err(PluginNetworkErrorCode::NetworkTargetDenied);
                }
                if response.status == 303 {
                    method = PluginNetworkRequestMethod::Get;
                    body.clear();
                    content_type = None;
                }
                redirects += 1;
                url = next;
                continue;
            }
            return parse_response(response, normalized_headers);
        }
    }

    pub(super) fn cancel_context(&self, context: &PluginNetworkContextIdentity) -> usize {
        self.registry.cancel_context(context)
    }

    pub(super) fn cancel_request_context(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        request_id: &str,
    ) -> usize {
        self.registry
            .cancel_request_context(plugin_id, plugin_generation, request_id)
    }

    pub(super) fn cancel_runtime(
        &self,
        plugin_id: &str,
        plugin_generation: u64,
        activation_id: u64,
        admission_epoch: u64,
    ) -> usize {
        self.registry
            .cancel_runtime(plugin_id, plugin_generation, activation_id, admission_epoch)
    }

    pub(super) fn cancel_generation(&self, plugin_id: &str, plugin_generation: u64) -> usize {
        self.registry
            .cancel_generation(plugin_id, plugin_generation)
    }

    pub(super) fn publish_plugin_authority(
        &self,
        plugin_id: &str,
        retained: Option<(u64, u64, u64)>,
    ) -> usize {
        self.registry.cancel_plugin_except(plugin_id, retained)
    }

    pub(super) fn shutdown(&self) -> usize {
        self.registry.shutdown()
    }

    #[cfg(test)]
    pub(super) fn global_active_for_test(&self) -> usize {
        self.registry.global_active_for_test()
    }

    #[cfg(test)]
    async fn request_for_test(
        &self,
        context: &PluginNetworkContextIdentity,
        authorized_hosts: &BTreeSet<String>,
        request: PluginNetworkRequest,
    ) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
        self.execute(
            self.admit(context, authorized_hosts, request)?,
            Arc::new(|_, _| true),
        )
        .await?
        .deliver()
    }
}

impl PluginNetworkErrorCode {
    fn log_category(self) -> &'static str {
        match self {
            Self::InvalidNetworkRequest => "invalid-request",
            Self::PermissionDenied => "permission-denied",
            Self::NetworkTargetDenied => "target-denied",
            Self::NetworkTimeout => "timeout",
            Self::NetworkFailure => "network-failure",
            Self::NetworkResponseTooLarge => "response-too-large",
            Self::NetworkResponseInvalid => "response-invalid",
            Self::NetworkLimitExceeded => "limit-exceeded",
            Self::ExpiredRequest => "expired",
        }
    }
}

fn validate_request(
    request: PluginNetworkRequest,
    authorized_hosts: &BTreeSet<String>,
) -> Result<ValidatedRequest, PluginNetworkErrorCode> {
    let url = parse_initial_url(&request.url)?;
    let hostname = validate_target(&url, authorized_hosts)?;
    let plugin_headers = validate_request_headers(request.headers)?;
    let (body, content_type) = encode_body(request.method, request.body)?;
    Ok(ValidatedRequest {
        url,
        hostname,
        method: request.method,
        plugin_headers,
        body,
        content_type,
        authorized_hosts: authorized_hosts.clone(),
        deadline: Instant::now() + TOTAL_DEADLINE,
        started: StdInstant::now(),
    })
}

fn parse_initial_url(raw: &str) -> Result<Url, PluginNetworkErrorCode> {
    if raw.as_bytes().len() > MAX_URL_BYTES {
        return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
    }
    Url::parse(raw).map_err(|_| PluginNetworkErrorCode::InvalidNetworkRequest)
}

fn validate_target(
    url: &Url,
    authorized_hosts: &BTreeSet<String>,
) -> Result<String, PluginNetworkErrorCode> {
    let userinfo = &url[url::Position::BeforeUsername..url::Position::BeforeHost];
    if url.as_str().as_bytes().len() > MAX_URL_BYTES
        || url.scheme() != "https"
        || !userinfo.is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return Err(PluginNetworkErrorCode::NetworkTargetDenied);
    }
    let hostname = url
        .domain()
        .map(str::to_owned)
        .ok_or(PluginNetworkErrorCode::NetworkTargetDenied)?;
    if !authorized_hosts.contains(&hostname) {
        return Err(PluginNetworkErrorCode::NetworkTargetDenied);
    }
    Ok(hostname)
}

fn validate_request_headers(
    headers: Option<BTreeMap<String, String>>,
) -> Result<Vec<(String, Vec<u8>)>, PluginNetworkErrorCode> {
    let headers = headers.unwrap_or_default();
    if headers.len() > MAX_REQUEST_HEADER_FIELDS {
        return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
    }
    let mut total = 0usize;
    let mut normalized = BTreeMap::new();
    for (name, value) in headers {
        total = total
            .checked_add(name.as_bytes().len())
            .and_then(|total| total.checked_add(value.as_bytes().len()))
            .ok_or(PluginNetworkErrorCode::InvalidNetworkRequest)?;
        if total > MAX_REQUEST_HEADER_BYTES {
            return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
        }
        let parsed = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| PluginNetworkErrorCode::InvalidNetworkRequest)?;
        let canonical = parsed.as_str().to_owned();
        if protected_request_header(&canonical)
            || value
                .as_bytes()
                .iter()
                .any(|byte| (*byte < 0x20 && *byte != b'\t') || *byte == 0x7f)
            || hyper::header::HeaderValue::from_bytes(value.as_bytes()).is_err()
            || normalized.insert(canonical, value.into_bytes()).is_some()
        {
            return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
        }
    }
    Ok(normalized.into_iter().collect())
}

fn protected_request_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "content-type"
            | "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
            | "cookie"
            | "origin"
            | "referer"
            | "user-agent"
            | "accept-encoding"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "forwarded"
            | "via"
    ) || name.starts_with("proxy-")
        || name.starts_with("sec-")
        || name.starts_with("x-forwarded-")
}

fn encode_body(
    method: PluginNetworkRequestMethod,
    body: Option<PluginNetworkRequestBody>,
) -> Result<(Vec<u8>, Option<&'static str>), PluginNetworkErrorCode> {
    if method == PluginNetworkRequestMethod::Get && body.is_some() {
        return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
    }
    let (body, content_type) = match body {
        None => (Vec::new(), None),
        Some(PluginNetworkRequestBody::Json { value }) => (
            serde_json::to_vec(&value)
                .map_err(|_| PluginNetworkErrorCode::InvalidNetworkRequest)?,
            Some("application/json; charset=utf-8"),
        ),
        Some(PluginNetworkRequestBody::Text { value }) => {
            (value.into_bytes(), Some("text/plain; charset=utf-8"))
        }
        Some(PluginNetworkRequestBody::Form { value }) => {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in value {
                serializer.append_pair(&key, &value);
            }
            (
                serializer.finish().into_bytes(),
                Some("application/x-www-form-urlencoded; charset=utf-8"),
            )
        }
    };
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err(PluginNetworkErrorCode::InvalidNetworkRequest);
    }
    Ok((body, content_type))
}

fn transport_headers(
    plugin_headers: &[(String, Vec<u8>)],
    body: &[u8],
    content_type: Option<&str>,
) -> Vec<(String, Vec<u8>)> {
    let mut headers = plugin_headers.to_vec();
    headers.push(("accept-encoding".into(), b"identity".to_vec()));
    headers.push(("user-agent".into(), USER_AGENT.as_bytes().to_vec()));
    headers.push(("content-length".into(), body.len().to_string().into_bytes()));
    if let Some(content_type) = content_type {
        headers.push(("content-type".into(), content_type.as_bytes().to_vec()));
    }
    headers
}

fn path_and_query(url: &Url) -> String {
    let mut value = if url.path().is_empty() {
        "/".to_owned()
    } else {
        url.path().to_owned()
    };
    if let Some(query) = url.query() {
        value.push('?');
        value.push_str(query);
    }
    value
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn single_location(headers: &[(String, String)]) -> Result<&str, PluginNetworkErrorCode> {
    let locations = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("location"))
        .collect::<Vec<_>>();
    if locations.len() != 1 {
        return Err(PluginNetworkErrorCode::NetworkTargetDenied);
    }
    Ok(locations[0].1.as_str())
}

fn validate_response_metadata(
    response: &NativeHttpsResponse,
) -> Result<Vec<(String, String)>, PluginNetworkErrorCode> {
    if !(100..=999).contains(&response.status) {
        return Err(PluginNetworkErrorCode::NetworkResponseInvalid);
    }
    if response.headers.len() > MAX_RESPONSE_HEADER_FIELDS
        || response.body.len() > MAX_RESPONSE_BODY_BYTES
    {
        return Err(PluginNetworkErrorCode::NetworkResponseTooLarge);
    }
    let mut total = 0usize;
    let mut named = Vec::with_capacity(response.headers.len());
    for (name, value) in &response.headers {
        total = total
            .checked_add(name.as_bytes().len())
            .and_then(|total| total.checked_add(value.len()))
            .ok_or(PluginNetworkErrorCode::NetworkResponseTooLarge)?;
        if total > MAX_RESPONSE_HEADER_BYTES {
            return Err(PluginNetworkErrorCode::NetworkResponseTooLarge);
        }
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?
            .as_str()
            .to_owned();
        named.push((name, value));
    }

    let mut connection_headers = BTreeSet::new();
    for (name, value) in &named {
        if name == "content-encoding" {
            let value = std::str::from_utf8(value)
                .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?;
            if !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("identity") {
                return Err(PluginNetworkErrorCode::NetworkResponseInvalid);
            }
        }
        if name == "connection" {
            let value = std::str::from_utf8(value)
                .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?;
            for token in value
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                let token = HeaderName::from_bytes(token.as_bytes())
                    .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?;
                connection_headers.insert(token.as_str().to_owned());
            }
        }
    }

    let mut normalized = Vec::with_capacity(named.len());
    for (name, value) in named {
        if protected_response_header(&name) || connection_headers.contains(&name) {
            continue;
        }
        let value = String::from_utf8(value.clone())
            .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?;
        normalized.push((name, value));
    }
    Ok(normalized)
}

fn parse_response(
    response: NativeHttpsResponse,
    normalized: Vec<(String, String)>,
) -> Result<PluginNetworkResponse, PluginNetworkErrorCode> {
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for (name, value) in normalized {
        headers.entry(name).or_default().push(value);
    }
    let body = String::from_utf8(response.body)
        .map_err(|_| PluginNetworkErrorCode::NetworkResponseInvalid)?;
    Ok(PluginNetworkResponse {
        status: response.status,
        headers,
        body,
    })
}

fn protected_response_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "set-cookie"
            | "set-cookie2"
    )
}

fn map_registry_error(error: PluginNetworkRegistryError) -> PluginNetworkErrorCode {
    match error {
        PluginNetworkRegistryError::LimitExceeded | PluginNetworkRegistryError::Exhausted => {
            PluginNetworkErrorCode::NetworkLimitExceeded
        }
        PluginNetworkRegistryError::Expired => PluginNetworkErrorCode::ExpiredRequest,
        PluginNetworkRegistryError::InvalidIdentity | PluginNetworkRegistryError::Unavailable => {
            PluginNetworkErrorCode::NetworkFailure
        }
    }
}

fn map_dns_error(error: DnsResolveError) -> PluginNetworkErrorCode {
    match error {
        DnsResolveError::Busy => PluginNetworkErrorCode::NetworkLimitExceeded,
        DnsResolveError::Cancelled => PluginNetworkErrorCode::ExpiredRequest,
        DnsResolveError::Timeout => PluginNetworkErrorCode::NetworkTimeout,
        DnsResolveError::Empty | DnsResolveError::Denied => {
            PluginNetworkErrorCode::NetworkTargetDenied
        }
        DnsResolveError::Failed => PluginNetworkErrorCode::NetworkFailure,
    }
}

fn map_transport_error(error: NativeTransportError) -> PluginNetworkErrorCode {
    match error {
        NativeTransportError::Timeout => PluginNetworkErrorCode::NetworkTimeout,
        NativeTransportError::Cancelled => PluginNetworkErrorCode::ExpiredRequest,
        NativeTransportError::ResponseHeadersTooLarge
        | NativeTransportError::ResponseBodyTooLarge => {
            PluginNetworkErrorCode::NetworkResponseTooLarge
        }
        NativeTransportError::InvalidRequest => PluginNetworkErrorCode::InvalidNetworkRequest,
        NativeTransportError::Protocol => PluginNetworkErrorCode::NetworkResponseInvalid,
        NativeTransportError::Connect
        | NativeTransportError::Tls
        | NativeTransportError::Network => PluginNetworkErrorCode::NetworkFailure,
    }
}

fn log_terminal(
    identity: &PluginNetworkCallIdentity,
    method: PluginNetworkRequestMethod,
    hostname: &str,
    result: &str,
    status: Option<u16>,
    started: StdInstant,
) {
    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    eprintln!(
        "[plugin-network] plugin={} method={} host={} result={} status={} elapsedMs={}",
        identity.context.plugin_id,
        method.as_log(),
        hostname,
        result,
        status.map_or_else(|| "none".to_owned(), |status| status.to_string()),
        elapsed_ms
    );
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        net::SocketAddr,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use hyper::Method;
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use super::{
        PluginHttpsBroker, PluginNetworkErrorCode, PluginNetworkRequest, PluginNetworkRequestBody,
        PluginNetworkRequestMethod,
    };
    use crate::public_plugins::network::{
        registry::{PluginNetworkContextIdentity, PluginNetworkRequestRegistry},
        transport::{
            BoundedDnsResolver, DeterministicHttpsTransport, HttpsTransport, HttpsTransportFuture,
            NativeHttpsRequest, NativeHttpsResponse, NativeTransportError,
        },
    };

    struct BlockingTransport {
        started: Arc<Notify>,
    }

    impl HttpsTransport for BlockingTransport {
        fn execute<'a>(
            &'a self,
            _request: NativeHttpsRequest,
            cancellation: &'a CancellationToken,
        ) -> HttpsTransportFuture<'a> {
            Box::pin(async move {
                self.started.notify_one();
                cancellation.cancelled().await;
                Err(NativeTransportError::Cancelled)
            })
        }
    }

    fn context(request_id: &str) -> PluginNetworkContextIdentity {
        PluginNetworkContextIdentity::new("com.example.network", 1, 2, 3, request_id).unwrap()
    }

    fn hosts() -> BTreeSet<String> {
        BTreeSet::from(["api.example.com".into()])
    }

    fn response(status: u16, headers: Vec<(&str, &[u8])>, body: &[u8]) -> NativeHttpsResponse {
        NativeHttpsResponse {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.into(), value.to_vec()))
                .collect(),
            body: body.to_vec(),
        }
    }

    fn broker(
        outcomes: Vec<Result<NativeHttpsResponse, NativeTransportError>>,
    ) -> (PluginHttpsBroker, Arc<DeterministicHttpsTransport>) {
        let transport = Arc::new(DeterministicHttpsTransport::new(outcomes));
        let resolver = BoundedDnsResolver::with_lookup(1, 4, |_host, port| {
            Ok(vec![SocketAddr::from(([8, 8, 8, 8], port))])
        });
        (
            PluginHttpsBroker::with_dependencies(
                PluginNetworkRequestRegistry::default(),
                resolver,
                transport.clone(),
            ),
            transport,
        )
    }

    fn request(method: PluginNetworkRequestMethod) -> PluginNetworkRequest {
        PluginNetworkRequest {
            url: "https://api.example.com/translate?q=hello".into(),
            method,
            headers: None,
            body: None,
        }
    }

    #[tokio::test]
    async fn plugin_https_broker_encodes_get_and_all_post_body_forms_exactly() {
        let (broker, transport) = broker(vec![
            Ok(response(200, vec![], b"ok")),
            Ok(response(200, vec![], b"ok")),
            Ok(response(200, vec![], b"ok")),
            Ok(response(200, vec![], b"ok")),
        ]);
        let cases = [
            request(PluginNetworkRequestMethod::Get),
            PluginNetworkRequest {
                body: Some(PluginNetworkRequestBody::Json {
                    value: json!({"text": "Hello"}),
                }),
                ..request(PluginNetworkRequestMethod::Post)
            },
            PluginNetworkRequest {
                body: Some(PluginNetworkRequestBody::Text {
                    value: "Hello".into(),
                }),
                ..request(PluginNetworkRequestMethod::Post)
            },
            PluginNetworkRequest {
                body: Some(PluginNetworkRequestBody::Form {
                    value: [("space key".into(), "a+b&c".into())].into_iter().collect(),
                }),
                ..request(PluginNetworkRequestMethod::Post)
            },
        ];
        for (index, request) in cases.into_iter().enumerate() {
            broker
                .request_for_test(&context(&format!("request-{index}")), &hosts(), request)
                .await
                .unwrap();
        }

        let sent = transport.take_requests();
        assert_eq!(sent[0].method, Method::GET);
        assert_eq!(sent[0].body, b"");
        assert_eq!(sent[1].body, br#"{"text":"Hello"}"#);
        assert_eq!(sent[2].body, b"Hello");
        assert_eq!(sent[3].body, b"space+key=a%2Bb%26c");
        let content_types = sent
            .iter()
            .map(|request| {
                request
                    .headers
                    .iter()
                    .find(|(name, _)| name == "content-type")
                    .map(|(_, value)| String::from_utf8(value.clone()).unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            content_types,
            vec![
                None,
                Some("application/json; charset=utf-8".into()),
                Some("text/plain; charset=utf-8".into()),
                Some("application/x-www-form-urlencoded; charset=utf-8".into()),
            ]
        );
        assert!(sent.iter().all(|request| request
            .headers
            .iter()
            .any(|(name, value)| name == "accept-encoding" && value == b"identity")));
    }

    #[tokio::test]
    async fn plugin_https_broker_rejects_url_headers_body_and_limits_after_attempt() {
        let (broker, transport) = broker(Vec::new());
        let invalid = [
            PluginNetworkRequest {
                url: "http://api.example.com/".into(),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                url: "https://other.example.com/".into(),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                url: "https://user@api.example.com/".into(),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                url: "https://api.example.com:444/".into(),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                url: "https://api.example.com/#fragment".into(),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                headers: Some([("Host".into(), "evil.example.com".into())].into()),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                headers: Some(
                    [
                        ("X-Key".into(), "one".into()),
                        ("x-key".into(), "two".into()),
                    ]
                    .into(),
                ),
                ..request(PluginNetworkRequestMethod::Get)
            },
            PluginNetworkRequest {
                body: Some(PluginNetworkRequestBody::Text {
                    value: "body".into(),
                }),
                ..request(PluginNetworkRequestMethod::Get)
            },
        ];
        for (index, request) in invalid.into_iter().enumerate() {
            let error = broker
                .request_for_test(&context(&format!("invalid-{index}")), &hosts(), request)
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                PluginNetworkErrorCode::InvalidNetworkRequest
                    | PluginNetworkErrorCode::NetworkTargetDenied
            ));
        }
        let oversized = PluginNetworkRequest {
            body: Some(PluginNetworkRequestBody::Text {
                value: "x".repeat(64 * 1024 + 1),
            }),
            ..request(PluginNetworkRequestMethod::Post)
        };
        assert_eq!(
            broker
                .request_for_test(&context("oversized"), &hosts(), oversized)
                .await,
            Err(PluginNetworkErrorCode::InvalidNetworkRequest)
        );
        assert!(transport.take_requests().is_empty());
    }

    #[tokio::test]
    async fn plugin_https_broker_policy_failures_consume_attempts_but_not_concurrency() {
        let (broker, transport) = broker(Vec::new());
        let identity = context("attempt-limit");
        for _ in 0..8 {
            let invalid = PluginNetworkRequest {
                url: "http://api.example.com/".into(),
                ..request(PluginNetworkRequestMethod::Get)
            };
            assert_eq!(
                broker.request_for_test(&identity, &hosts(), invalid).await,
                Err(PluginNetworkErrorCode::NetworkTargetDenied)
            );
        }
        assert_eq!(
            broker
                .request_for_test(
                    &identity,
                    &hosts(),
                    request(PluginNetworkRequestMethod::Get),
                )
                .await,
            Err(PluginNetworkErrorCode::NetworkLimitExceeded)
        );
        assert!(transport.take_requests().is_empty());
    }

    #[tokio::test]
    async fn plugin_https_broker_enforces_request_header_field_and_byte_limits() {
        let (broker, transport) = broker(Vec::new());
        let too_many = PluginNetworkRequest {
            headers: Some(
                (0..33)
                    .map(|index| (format!("x-field-{index}"), "value".into()))
                    .collect(),
            ),
            ..request(PluginNetworkRequestMethod::Get)
        };
        assert_eq!(
            broker
                .request_for_test(&context("header-count"), &hosts(), too_many)
                .await,
            Err(PluginNetworkErrorCode::InvalidNetworkRequest)
        );

        let too_large = PluginNetworkRequest {
            headers: Some([("x-large".into(), "x".repeat(16 * 1024))].into()),
            ..request(PluginNetworkRequestMethod::Get)
        };
        assert_eq!(
            broker
                .request_for_test(&context("header-bytes"), &hosts(), too_large)
                .await,
            Err(PluginNetworkErrorCode::InvalidNetworkRequest)
        );
        assert!(transport.take_requests().is_empty());
    }

    #[tokio::test]
    async fn plugin_https_broker_filters_response_and_keeps_http_errors_successful() {
        let (broker, _) = broker(vec![Ok(response(
            503,
            vec![
                ("x-result", b"one"),
                ("x-result", b"two"),
                ("set-cookie", &[0xff]),
                ("proxy-authenticate", &[0xfe]),
                ("connection", b"x-private"),
                ("x-private", &[0xfd]),
            ],
            b"later",
        ))]);
        let response = broker
            .request_for_test(
                &context("response"),
                &hosts(),
                request(PluginNetworkRequestMethod::Get),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(response.body, "later");
        assert_eq!(response.headers.get("x-result").unwrap(), &["one", "two"]);
        assert!(!response.headers.contains_key("set-cookie"));
        assert!(!response.headers.contains_key("proxy-authenticate"));
        assert!(!response.headers.contains_key("connection"));
        assert!(!response.headers.contains_key("x-private"));
    }

    #[tokio::test]
    async fn plugin_https_broker_rejects_invalid_encoding_utf8_and_oversized_response() {
        let cases = [
            response(200, vec![("content-encoding", b"gzip")], b"ok"),
            response(
                200,
                vec![
                    ("connection", b"content-encoding"),
                    ("content-encoding", b"br"),
                ],
                b"ok",
            ),
            response(200, vec![], &[0xff]),
            response(200, vec![], &vec![b'x'; 1024 * 1024 + 1]),
        ];
        let expected = [
            PluginNetworkErrorCode::NetworkResponseInvalid,
            PluginNetworkErrorCode::NetworkResponseInvalid,
            PluginNetworkErrorCode::NetworkResponseInvalid,
            PluginNetworkErrorCode::NetworkResponseTooLarge,
        ];
        for (index, (response, expected)) in cases.into_iter().zip(expected).enumerate() {
            let (broker, _) = broker(vec![Ok(response)]);
            assert_eq!(
                broker
                    .request_for_test(
                        &context(&format!("invalid-response-{index}")),
                        &hosts(),
                        request(PluginNetworkRequestMethod::Get),
                    )
                    .await,
                Err(expected)
            );
        }
    }

    #[tokio::test]
    async fn plugin_https_broker_enforces_deterministic_response_header_limits() {
        let too_many = (0..65)
            .map(|index| (format!("x-field-{index}"), b"value".to_vec()))
            .collect::<Vec<_>>();
        let too_large = vec![("x-large".into(), vec![b'x'; 32 * 1024])];
        for (index, headers) in [too_many, too_large].into_iter().enumerate() {
            let (broker, _) = broker(vec![Ok(NativeHttpsResponse {
                status: 200,
                headers,
                body: Vec::new(),
            })]);
            assert_eq!(
                broker
                    .request_for_test(
                        &context(&format!("response-headers-{index}")),
                        &hosts(),
                        request(PluginNetworkRequestMethod::Get),
                    )
                    .await,
                Err(PluginNetworkErrorCode::NetworkResponseTooLarge)
            );
        }
    }

    #[tokio::test]
    async fn plugin_https_broker_follows_three_same_host_redirects_and_revalidates_each_hop() {
        let (broker, transport) = broker(vec![
            Ok(response(302, vec![("location", b"/two")], b"")),
            Ok(response(307, vec![("location", b"/three")], b"")),
            Ok(response(303, vec![("location", b"/final")], b"")),
            Ok(response(200, vec![], b"done")),
        ]);
        let checked = Arc::new(Mutex::new(Vec::new()));
        let prepared = broker
            .admit(
                &context("redirects"),
                &hosts(),
                PluginNetworkRequest {
                    body: Some(PluginNetworkRequestBody::Text {
                        value: "payload".into(),
                    }),
                    ..request(PluginNetworkRequestMethod::Post)
                },
            )
            .unwrap();
        let pending = broker
            .execute(prepared, {
                let checked = checked.clone();
                Arc::new(move |_identity, host| {
                    checked.lock().unwrap().push(host.to_owned());
                    true
                })
            })
            .await
            .unwrap();
        let response = pending.deliver().unwrap();
        assert_eq!(response.body, "done");
        assert_eq!(checked.lock().unwrap().as_slice(), ["api.example.com"; 3]);
        let sent = transport.take_requests();
        assert_eq!(sent.len(), 4);
        assert!(sent
            .iter()
            .all(|request| request.deadline == sent[0].deadline));
        assert_eq!(sent[0].method, Method::POST);
        assert_eq!(sent[1].body, b"payload");
        assert_eq!(sent[3].method, Method::GET);
        assert!(sent[3].body.is_empty());
    }

    #[tokio::test]
    async fn plugin_https_broker_rejects_cross_host_and_fourth_redirect_without_forwarding() {
        let (cross_host, cross_transport) = broker(vec![Ok(response(
            302,
            vec![("location", b"https://other.example.com/steal")],
            b"",
        ))]);
        let mut secret = request(PluginNetworkRequestMethod::Post);
        secret.headers = Some([("authorization".into(), "Bearer secret".into())].into());
        secret.body = Some(PluginNetworkRequestBody::Text {
            value: "private".into(),
        });
        assert_eq!(
            cross_host
                .request_for_test(&context("cross-host"), &hosts(), secret)
                .await,
            Err(PluginNetworkErrorCode::NetworkTargetDenied)
        );
        assert_eq!(cross_transport.take_requests().len(), 1);

        let (too_many, transport) = broker(vec![
            Ok(response(301, vec![("location", b"/2")], b"")),
            Ok(response(302, vec![("location", b"/3")], b"")),
            Ok(response(307, vec![("location", b"/4")], b"")),
            Ok(response(308, vec![("location", b"/5")], b"")),
        ]);
        assert_eq!(
            too_many
                .request_for_test(
                    &context("too-many-redirects"),
                    &hosts(),
                    request(PluginNetworkRequestMethod::Get),
                )
                .await,
            Err(PluginNetworkErrorCode::NetworkTargetDenied)
        );
        assert_eq!(transport.take_requests().len(), 4);
    }

    #[tokio::test]
    async fn plugin_https_broker_stops_redirect_before_dns_when_authority_expires() {
        let (broker, transport) =
            broker(vec![Ok(response(302, vec![("location", b"/next")], b""))]);
        let prepared = broker
            .admit(
                &context("redirect-expired"),
                &hosts(),
                request(PluginNetworkRequestMethod::Get),
            )
            .unwrap();
        assert_eq!(
            broker.execute(prepared, Arc::new(|_, _| false)).await.err(),
            Some(PluginNetworkErrorCode::ExpiredRequest)
        );
        assert_eq!(transport.take_requests().len(), 1);
    }

    #[tokio::test]
    async fn plugin_https_broker_rejects_encoded_redirect_before_next_hop() {
        let (broker, transport) = broker(vec![Ok(response(
            302,
            vec![("location", b"/next"), ("content-encoding", b"gzip")],
            b"",
        ))]);
        assert_eq!(
            broker
                .request_for_test(
                    &context("encoded-redirect"),
                    &hosts(),
                    request(PluginNetworkRequestMethod::Get),
                )
                .await,
            Err(PluginNetworkErrorCode::NetworkResponseInvalid)
        );
        assert_eq!(transport.take_requests().len(), 1);
    }

    #[tokio::test]
    async fn plugin_https_broker_maps_transport_failures_without_private_detail() {
        let cases = [
            (
                NativeTransportError::Timeout,
                PluginNetworkErrorCode::NetworkTimeout,
            ),
            (
                NativeTransportError::Cancelled,
                PluginNetworkErrorCode::ExpiredRequest,
            ),
            (
                NativeTransportError::ResponseHeadersTooLarge,
                PluginNetworkErrorCode::NetworkResponseTooLarge,
            ),
            (
                NativeTransportError::Tls,
                PluginNetworkErrorCode::NetworkFailure,
            ),
            (
                NativeTransportError::Network,
                PluginNetworkErrorCode::NetworkFailure,
            ),
        ];
        for (index, (transport_error, expected)) in cases.into_iter().enumerate() {
            let (broker, _) = broker(vec![Err(transport_error)]);
            assert_eq!(
                broker
                    .request_for_test(
                        &context(&format!("transport-{index}")),
                        &hosts(),
                        request(PluginNetworkRequestMethod::Get),
                    )
                    .await,
                Err(expected)
            );
        }
    }

    #[tokio::test]
    async fn plugin_https_broker_holds_no_registry_lock_while_transport_is_pending() {
        let started = Arc::new(Notify::new());
        let resolver = BoundedDnsResolver::with_lookup(1, 1, |_host, port| {
            Ok(vec![SocketAddr::from(([8, 8, 8, 8], port))])
        });
        let broker = Arc::new(PluginHttpsBroker::with_dependencies(
            PluginNetworkRequestRegistry::default(),
            resolver,
            Arc::new(BlockingTransport {
                started: started.clone(),
            }),
        ));
        let identity = context("blocking");
        let prepared = broker
            .admit(
                &identity,
                &hosts(),
                request(PluginNetworkRequestMethod::Get),
            )
            .unwrap();
        let pending = tokio::spawn({
            let broker = broker.clone();
            async move { broker.execute(prepared, Arc::new(|_, _| true)).await }
        });
        tokio::time::timeout(Duration::from_millis(500), started.notified())
            .await
            .unwrap();
        assert_eq!(broker.cancel_context(&identity), 1);
        assert_eq!(
            pending.await.unwrap().err(),
            Some(PluginNetworkErrorCode::ExpiredRequest)
        );
    }
}
