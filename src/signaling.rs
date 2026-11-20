//! proxies BDS signaling and rewrites public UDP addresses in SDP answers.

use crate::{config::Config, session::SessionTable};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, OriginalUri, Path, State},
    http::{HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    routing::{any, post},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;

// caps both inbound SDP offers and backend responses.
const MAX_SDP_SIZE: usize = 1 << 20;

/// holds the backend client, instance configuration, and session table.
struct AppState {
    config: Config,
    client: reqwest::Client,
    sessions: Arc<SessionTable>,
}

/// serves signaling requests for one BDS instance.
pub async fn serve(
    listener: TcpListener,
    config: Config,
    sessions: Arc<SessionTable>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = Arc::new(AppState {
        config,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()?,
        sessions,
    });
    let app = Router::new()
        .route("/v1/join", any(join_info))
        .route("/v1/join/{network_id}", post(join))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_SDP_SIZE))
        .with_state(state);
    axum::serve(listener, app).await?;
    Ok(())
}

/// forwards the capability request and preserves its query string.
async fn join_info(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
) -> Response<Body> {
    let mut url = match backend_url(&state.config, None) {
        Ok(url) => url,
        Err(error) => return text_response(StatusCode::BAD_GATEWAY, error),
    };
    if let Some(query) = uri.query() {
        url.set_query(Some(query));
    }

    match state.client.get(url).send().await {
        Ok(response) => backend_response(response).await,
        Err(error) => text_response(StatusCode::BAD_GATEWAY, format!("BDS backend: {error}")),
    }
}

/// forwards an SDP offer, registers its ICE route, and rewrites the answer.
async fn join(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<String>,
    offer: Bytes,
) -> Response<Body> {
    if offer.is_empty() {
        return text_response(StatusCode::BAD_REQUEST, "empty SDP offer");
    }
    let offer_text = match std::str::from_utf8(&offer) {
        Ok(text) => text,
        Err(_) => return text_response(StatusCode::BAD_REQUEST, "SDP offer is not UTF-8"),
    };
    let client_ufrag = sdp_value(offer_text, "a=ice-ufrag:");
    if client_ufrag.is_empty() {
        return text_response(StatusCode::BAD_REQUEST, "SDP offer is missing ICE ufrag");
    }

    let url = match backend_url(&state.config, Some(&network_id)) {
        Ok(url) => url,
        Err(error) => return text_response(StatusCode::BAD_GATEWAY, error),
    };
    let backend = match state
        .client
        .post(url)
        .header(CONTENT_TYPE, "application/sdp")
        .body(offer)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return text_response(StatusCode::BAD_GATEWAY, format!("BDS backend: {error}"));
        }
    };

    let status = backend.status();
    let content_type = content_type(&backend);
    let answer = match read_bounded(backend).await {
        Ok(answer) => answer,
        Err(error) => return text_response(StatusCode::BAD_GATEWAY, error),
    };
    // preserve backend errors without trying to interpret them as SDP.
    if !status.is_success() || !content_type.to_ascii_lowercase().contains("sdp") {
        return raw_response(status, &content_type, answer);
    }

    let answer_text = match std::str::from_utf8(&answer) {
        Ok(text) => text,
        Err(_) => return text_response(StatusCode::BAD_GATEWAY, "BDS SDP answer is not UTF-8"),
    };
    let server_ufrag = sdp_value(answer_text, "a=ice-ufrag:");
    let ice_password = sdp_value(answer_text, "a=ice-pwd:");
    let bds_targets = match candidate_targets(answer_text, &state.config.bds_host).await {
        Ok(targets) => targets,
        Err(error) => return text_response(StatusCode::BAD_GATEWAY, error),
    };
    // register the route before returning an answer that advertises the proxy.
    if let Err(error) = state.sessions.register(
        network_id.clone(),
        client_ufrag,
        server_ufrag,
        ice_password,
        bds_targets,
    ) {
        return text_response(StatusCode::BAD_GATEWAY, error);
    }

    let rewritten = rewrite_sdp_addresses(
        answer_text,
        &state.config.public_host,
        state.config.public_port,
    );
    raw_response(status, &content_type, Bytes::from(rewritten.into_bytes()))
}

/// forwards a bounded backend response without changing its status or content type.
async fn backend_response(response: reqwest::Response) -> Response<Body> {
    let status = response.status();
    let content_type = content_type(&response);
    match read_bounded(response).await {
        Ok(body) => raw_response(status, &content_type, body),
        Err(error) => text_response(StatusCode::BAD_GATEWAY, error),
    }
}

/// reads a backend body while enforcing the configured size limit.
async fn read_bounded(mut response: reqwest::Response) -> Result<Bytes, String> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("BDS response: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_SDP_SIZE {
            return Err("BDS response exceeds the 1 MiB size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

/// returns the backend response content type when it is valid text.
fn content_type(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

async fn not_found() -> Response<Body> {
    text_response(StatusCode::NOT_FOUND, "not found")
}

/// builds the BDS signaling URL and encodes the network ID as one path segment.
fn backend_url(config: &Config, network_id: Option<&str>) -> Result<reqwest::Url, String> {
    let authority = if config.bds_host.contains(':') {
        format!("[{}]:{}", config.bds_host, config.bds_tcp)
    } else {
        format!("{}:{}", config.bds_host, config.bds_tcp)
    };
    let mut url = reqwest::Url::parse(&format!("http://{authority}/v1/join"))
        .map_err(|error| format!("BDS address is invalid: {error}"))?;
    if let Some(network_id) = network_id {
        url.path_segments_mut()
            .map_err(|_| "Unable to construct BDS signaling URL".to_owned())?
            .push(network_id);
    }
    Ok(url)
}

/// extracts the first value for an SDP attribute.
fn sdp_value(sdp: &str, prefix: &str) -> String {
    sdp.lines()
        .find_map(|line| line.trim_end_matches('\r').strip_prefix(prefix))
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

/// resolves UDP candidates to destinations reachable from the proxy.
async fn candidate_targets(sdp: &str, bds_host: &str) -> Result<Vec<SocketAddr>, String> {
    let mut targets = Vec::new();
    for line in sdp.lines().map(|line| line.trim_end_matches('\r')) {
        let Some(candidate) = line.strip_prefix("a=candidate:") else {
            continue;
        };
        let fields: Vec<_> = candidate.split_whitespace().collect();
        if fields.len() < 8 || !fields[2].eq_ignore_ascii_case("udp") {
            continue;
        }
        let port = fields[5]
            .parse::<u16>()
            .map_err(|error| format!("BDS candidate UDP port is invalid: {error}"))?;
        if port == 0 {
            continue;
        }
        let candidate_ip = fields[4]
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>();
        let target = match candidate_ip {
            Ok(ip) if !ip.is_unspecified() => SocketAddr::new(ip, port),
            _ => {
                let mut resolved =
                    tokio::net::lookup_host((bds_host, port))
                        .await
                        .map_err(|error| {
                            format!("Unable to resolve BDS address {bds_host}: {error}")
                        })?;
                resolved
                    .next()
                    .ok_or_else(|| format!("BDS address {bds_host} has no usable IP"))?
            }
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    Ok(targets)
}

/// rewrites UDP candidates and address fields to the configured public endpoint.
fn rewrite_sdp_addresses(sdp: &str, public_host: &str, public_port: u16) -> String {
    let public_ip = public_host.parse::<IpAddr>().ok();
    let mut output = String::with_capacity(sdp.len());
    for line in sdp.split_inclusive('\n') {
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let content = line.trim_end_matches(['\r', '\n']);
        if let Some(candidate) = content.strip_prefix("a=candidate:") {
            let mut fields: Vec<String> = candidate.split_whitespace().map(str::to_owned).collect();
            if fields.len() >= 8 && fields[2].eq_ignore_ascii_case("udp") {
                fields[4] = public_host.to_owned();
                fields[5] = public_port.to_string();
                output.push_str("a=candidate:");
                output.push_str(&fields.join(" "));
                output.push_str(ending);
                continue;
            }
        }
        if let Some(connection) = content.strip_prefix("c=IN ")
            && let Some(public_ip) = public_ip
        {
            let address_type = if public_ip.is_ipv4() { "IP4" } else { "IP6" };
            let fields: Vec<_> = connection.split_whitespace().collect();
            if fields.len() >= 2 {
                output.push_str("c=IN ");
                output.push_str(address_type);
                output.push(' ');
                output.push_str(&public_ip.to_string());
                output.push_str(ending);
                continue;
            }
        }
        if let Some(media) = content.strip_prefix("m=application ") {
            let mut fields: Vec<String> = media.split_whitespace().map(str::to_owned).collect();
            if fields.len() >= 2 && fields[0] != "0" {
                fields[0] = public_port.to_string();
                output.push_str("m=application ");
                output.push_str(&fields.join(" "));
                output.push_str(ending);
                continue;
            }
        }
        output.push_str(line);
    }
    output
}

/// builds a response while preserving the backend status and content type.
fn raw_response(status: reqwest::StatusCode, content_type: &str, body: Bytes) -> Response<Body> {
    let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Ok(value) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
    response
}

/// builds a plain text response generated by the proxy.
fn text_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    let mut response = Response::new(Body::from(body.into()));
    *response.status_mut() = status;
    response
}
