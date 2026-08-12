use crate::ai::{AiCleanupProposal, AiRecommendedAction};
use crate::ai_contract::{
    validate_transport_response, AiPathMode, AiTransportRequest, AiTransportResponse,
};
use crate::ai_provider::{AiAnalysisProvider, AiAnalysisRequest, AiProviderError};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ANALYZE_PATH: &str = "/v1/analyze";
const MAX_HEADER_BYTES: usize = 16 * 1024;
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct LoopbackGatewayConfig {
    pub address: SocketAddr,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_response_bytes: usize,
}

impl LoopbackGatewayConfig {
    pub fn from_endpoint(endpoint: &str) -> Result<Self, AiProviderError> {
        let authority = endpoint
            .strip_prefix("http://")
            .ok_or_else(|| invalid("loopback gateway endpoint must use http://"))?
            .trim_end_matches('/');

        if authority.contains('/') {
            return Err(invalid(
                "loopback gateway endpoint must contain only scheme, numeric address, and port",
            ));
        }

        let address = SocketAddr::from_str(authority).map_err(|_| {
            invalid("loopback gateway endpoint must use a numeric IP address and explicit port")
        })?;
        if !address.ip().is_loopback() {
            return Err(invalid(
                "loopback gateway endpoint must resolve to a loopback IP",
            ));
        }
        if address.port() == 0 {
            return Err(invalid(
                "loopback gateway endpoint must use a non-zero port",
            ));
        }

        Ok(Self {
            address,
            connect_timeout: Duration::from_secs(3),
            io_timeout: Duration::from_secs(15),
            max_response_bytes: 64 * 1024,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LoopbackGatewayProvider {
    config: LoopbackGatewayConfig,
}

impl LoopbackGatewayProvider {
    pub fn new(config: LoopbackGatewayConfig) -> Self {
        Self { config }
    }

    fn analyze_transport(
        &self,
        analysis: &AiAnalysisRequest,
    ) -> Result<AiCleanupProposal, AiProviderError> {
        if !analysis.observation_is_bound() {
            return Err(invalid("analysis observation digest is stale or invalid"));
        }

        let request = AiTransportRequest::new(next_request_id(), analysis.observation.clone());
        if request.candidate.observation.digest != analysis.observation_digest {
            return Err(invalid(
                "transport observation digest does not match analysis request",
            ));
        }

        let body = request_json(&request);
        let raw = self.post_json(body.as_bytes())?;
        let response_text = std::str::from_utf8(&raw)
            .map_err(|_| invalid("gateway response body is not valid UTF-8 JSON"))?;
        if !response_text.trim_start().starts_with('{') {
            return Err(invalid("gateway response body must be a JSON object"));
        }

        let response: AiTransportResponse = serde_yaml::from_str(response_text)
            .map_err(|error| invalid(format!("invalid gateway JSON response: {error}")))?;
        validate_transport_response(&request, response)
            .map_err(|error| invalid(format!("gateway response rejected: {error}")))
    }

    fn post_json(&self, body: &[u8]) -> Result<Vec<u8>, AiProviderError> {
        let mut stream =
            TcpStream::connect_timeout(&self.config.address, self.config.connect_timeout)
                .map_err(|error| unavailable(format!("connecting to loopback gateway: {error}")))?;
        stream
            .set_read_timeout(Some(self.config.io_timeout))
            .map_err(|error| unavailable(format!("setting gateway read timeout: {error}")))?;
        stream
            .set_write_timeout(Some(self.config.io_timeout))
            .map_err(|error| unavailable(format!("setting gateway write timeout: {error}")))?;

        let host = host_header(self.config.address);
        let headers = format!(
            "POST {ANALYZE_PATH} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|error| unavailable(format!("writing gateway request: {error}")))?;

        let hard_limit = MAX_HEADER_BYTES
            .saturating_add(self.config.max_response_bytes)
            .saturating_add(1);
        let mut raw = Vec::new();
        stream
            .take(hard_limit as u64)
            .read_to_end(&mut raw)
            .map_err(|error| unavailable(format!("reading gateway response: {error}")))?;
        if raw.len() >= hard_limit {
            return Err(invalid("gateway response exceeds configured size limit"));
        }

        parse_http_response(&raw, self.config.max_response_bytes)
    }
}

impl AiAnalysisProvider for LoopbackGatewayProvider {
    fn analyze(&self, request: &AiAnalysisRequest) -> Result<AiCleanupProposal, AiProviderError> {
        self.analyze_transport(request)
    }
}

fn parse_http_response(raw: &[u8], max_body_bytes: usize) -> Result<Vec<u8>, AiProviderError> {
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| invalid("gateway returned malformed HTTP headers"))?;
    if boundary > MAX_HEADER_BYTES {
        return Err(invalid("gateway response headers exceed size limit"));
    }

    let header_bytes = &raw[..boundary];
    let headers = std::str::from_utf8(header_bytes)
        .map_err(|_| invalid("gateway returned non-UTF-8 HTTP headers"))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .ok_or_else(|| invalid("gateway response is missing HTTP status"))?;
    if status != "HTTP/1.1 200 OK" && status != "HTTP/1.0 200 OK" {
        return Err(unavailable(format!(
            "gateway returned non-success status: {status}"
        )));
    }

    let mut content_length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(invalid("gateway returned malformed HTTP header"));
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") && !value.eq_ignore_ascii_case("identity")
        {
            return Err(invalid(
                "gateway transfer encoding is unsupported; redirects/chunking are disabled",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| invalid("gateway returned invalid Content-Length"))?;
            if content_length.replace(parsed).is_some() {
                return Err(invalid("gateway returned multiple Content-Length headers"));
            }
        }
    }

    let body = &raw[boundary + 4..];
    if body.len() > max_body_bytes {
        return Err(invalid(
            "gateway response body exceeds configured size limit",
        ));
    }
    if let Some(expected) = content_length {
        if expected != body.len() {
            return Err(invalid(
                "gateway response Content-Length does not match body",
            ));
        }
    }
    Ok(body.to_vec())
}

fn host_header(address: SocketAddr) -> String {
    match address.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", address.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", address.port()),
    }
}

fn request_json(request: &AiTransportRequest) -> String {
    let facts = &request.candidate.facts;
    let deterministic = &facts.deterministic;
    let actions = request
        .constraints
        .allowed_actions
        .iter()
        .map(|action| json_string(action_name(*action)))
        .collect::<Vec<_>>()
        .join(",");
    let labels = facts
        .labels
        .iter()
        .map(|label| json_string(label))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{\"contract_version\":{},\"request_id\":{},\"task\":{},",
            "\"candidate\":{{\"scan_id\":{},\"candidate_key\":{},\"path\":{},",
            "\"path_mode\":{},\"size_bytes\":{},\"age_seconds\":{},\"labels\":[{}],",
            "\"deterministic\":{{\"classification\":{},\"matched_rule\":{},",
            "\"protected\":{},\"max_allowed_risk\":{}}},\"adapter\":{},",
            "\"observation\":{{\"digest\":{}}}}},",
            "\"constraints\":{{\"allowed_actions\":[{}],\"file_contents_available\":{},",
            "\"mutation_authority\":{}}}}}"
        ),
        request.contract_version,
        json_string(&request.request_id),
        json_string(&request.task),
        facts.scan_id,
        json_string(&facts.candidate_key),
        json_string(&facts.path),
        json_string(path_mode_name(facts.path_mode)),
        facts.size_bytes,
        json_optional_u64(facts.age_seconds),
        labels,
        json_optional_string(deterministic.classification.as_deref()),
        json_optional_string(deterministic.matched_rule.as_deref()),
        deterministic.protected,
        json_string(&deterministic.max_allowed_risk),
        json_optional_string(facts.adapter.as_deref()),
        json_string(&request.candidate.observation.digest),
        actions,
        request.constraints.file_contents_available,
        request.constraints.mutation_authority,
    )
}

fn json_optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value <= '\u{1f}' => {
                output.push_str(&format!("\\u{:04x}", value as u32));
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

fn action_name(action: AiRecommendedAction) -> &'static str {
    match action {
        AiRecommendedAction::Ignore => "ignore",
        AiRecommendedAction::Review => "review",
        AiRecommendedAction::Quarantine => "quarantine",
    }
}

fn path_mode_name(mode: AiPathMode) -> &'static str {
    match mode {
        AiPathMode::Full => "full",
        AiPathMode::Basename => "basename",
        AiPathMode::Redacted => "redacted",
    }
}

fn next_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tidyfs-{nanos:x}-{counter:x}")
}

fn invalid(message: impl Into<String>) -> AiProviderError {
    AiProviderError::InvalidResponse(message.into())
}

fn unavailable(message: impl Into<String>) -> AiProviderError {
    AiProviderError::Unavailable(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_contract::{AiDeterministicFacts, AiObservation};
    use std::net::TcpListener;
    use std::thread;

    fn analysis_request() -> AiAnalysisRequest {
        AiAnalysisRequest::new(AiObservation {
            scan_id: 42,
            candidate_key: "scan-42:candidate-7".to_owned(),
            path: "/home/user/project \"one\"/.cache".to_owned(),
            path_mode: AiPathMode::Full,
            size_bytes: 1024,
            age_seconds: Some(3600),
            labels: vec!["cache".to_owned(), "generated_artifact".to_owned()],
            deterministic: AiDeterministicFacts {
                classification: Some("cache".to_owned()),
                matched_rule: None,
                protected: false,
                max_allowed_risk: "medium".to_owned(),
            },
            adapter: None,
        })
    }

    #[test]
    fn rejects_non_loopback_or_hostname_endpoints() {
        assert!(LoopbackGatewayConfig::from_endpoint("http://127.0.0.1:8080").is_ok());
        assert!(LoopbackGatewayConfig::from_endpoint("http://[::1]:8080").is_ok());
        assert!(LoopbackGatewayConfig::from_endpoint("http://localhost:8080").is_err());
        assert!(LoopbackGatewayConfig::from_endpoint("http://192.0.2.1:8080").is_err());
        assert!(LoopbackGatewayConfig::from_endpoint("https://127.0.0.1:8080").is_err());
        assert!(LoopbackGatewayConfig::from_endpoint("http://127.0.0.1:8080/other").is_err());
    }

    #[test]
    fn request_encoder_emits_parseable_json_with_exact_binding() {
        let analysis = analysis_request();
        let request = AiTransportRequest::new("req-1".to_owned(), analysis.observation.clone());
        let encoded = request_json(&request);
        assert!(encoded.starts_with('{'));
        let decoded: AiTransportRequest = serde_yaml::from_str(&encoded).expect("JSON parses");
        assert_eq!(decoded, request);
    }

    #[test]
    fn local_fake_gateway_round_trip_validates_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let address = listener.local_addr().expect("local address");
        let analysis = analysis_request();
        let expected_digest = analysis.observation_digest.clone();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).expect("read request");
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let boundary = request
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap();
                    let headers = std::str::from_utf8(&request[..boundary]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap();
                    if request.len() >= boundary + 4 + length {
                        break;
                    }
                }
            }
            let boundary = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap();
            let body = std::str::from_utf8(&request[boundary + 4..]).unwrap();
            let transport: AiTransportRequest = serde_yaml::from_str(body).expect("request JSON");
            assert_eq!(transport.candidate.observation.digest, expected_digest);

            let response = format!(
                concat!(
                    "{{\"contract_version\":1,\"request_id\":{},",
                    "\"proposal\":{{\"schema_version\":1,\"classification\":\"regenerable_cache\",",
                    "\"confidence\":0.91,\"rationale\":[\"generated cache\"],\"caveats\":[],",
                    "\"risk\":\"medium\",\"recommended_action\":\"review\",",
                    "\"provenance\":{{\"provider\":\"fake\",\"model\":\"test\",",
                    "\"request_id\":{}}}}},\"observation\":{{\"digest\":{}}}}}"
                ),
                json_string(&transport.request_id),
                json_string(&transport.request_id),
                json_string(&transport.candidate.observation.digest),
            );
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            stream.write_all(headers.as_bytes()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        });

        let provider = LoopbackGatewayProvider::new(LoopbackGatewayConfig {
            address,
            connect_timeout: Duration::from_secs(1),
            io_timeout: Duration::from_secs(1),
            max_response_bytes: 16 * 1024,
        });
        let proposal = provider.analyze(&analysis).expect("valid proposal");
        assert_eq!(proposal.classification, "regenerable_cache");
        assert_eq!(proposal.recommended_action, AiRecommendedAction::Review);
        server.join().unwrap();
    }

    #[test]
    fn rejects_redirect_and_oversized_response() {
        let redirect =
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_http_response(redirect, 1024).is_err());

        let oversized = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ntest";
        assert!(parse_http_response(oversized, 3).is_err());
    }
}
