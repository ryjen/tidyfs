use crate::ai::AiCleanupProposal;
use crate::ai_contract::{
    validate_transport_response, AiObservationBinding, AiTransportRequest, AiTransportResponse,
};
use crate::ai_goal::{
    validate_goal_response, AiGoalRecommendation, AiGoalRequest, AiGoalTransportResponse,
};
use crate::ai_provider::{AiAnalysisProvider, AiAnalysisRequest, AiProviderError};
use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ANALYZE_PATH: &str = "/v1/analyze";
const RECOMMEND_PATH: &str = "/v1/recommend";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const HEADER_TERMINATOR_BYTES: usize = 4;
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct LoopbackGatewayConfig {
    address: SocketAddr,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_request_bytes: usize,
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
            max_request_bytes: 32 * 1024,
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

    pub fn recommend_goal(
        &self,
        request: &AiGoalRequest,
    ) -> Result<AiGoalRecommendation, AiProviderError> {
        request
            .validate()
            .map_err(|error| invalid(format!("goal request rejected: {error}")))?;

        let body = serde_json::to_vec(request)
            .map_err(|error| invalid(format!("serializing goal gateway request: {error}")))?;
        if body.len() > self.config.max_request_bytes {
            return Err(invalid("gateway request exceeds configured size limit"));
        }

        let raw = self.post_json(RECOMMEND_PATH, &body)?;
        let response: AiGoalTransportResponse = serde_json::from_slice(&raw)
            .map_err(|error| invalid(format!("invalid goal gateway JSON response: {error}")))?;
        validate_goal_response(request, response)
            .map_err(|error| invalid(format!("goal gateway response rejected: {error}")))
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

        let body = serde_json::to_vec(&request)
            .map_err(|error| invalid(format!("serializing gateway JSON request: {error}")))?;
        if body.len() > self.config.max_request_bytes {
            return Err(invalid("gateway request exceeds configured size limit"));
        }

        let raw = self.post_json(ANALYZE_PATH, &body)?;
        let strict: StrictGatewayResponse = serde_json::from_slice(&raw)
            .map_err(|error| invalid(format!("invalid gateway JSON response: {error}")))?;
        if strict.proposal.provenance.request_id.as_deref() != Some(strict.request_id.as_str()) {
            return Err(invalid(
                "gateway proposal provenance request id does not match response request id",
            ));
        }

        let response = AiTransportResponse {
            contract_version: strict.contract_version,
            request_id: strict.request_id,
            proposal: strict.proposal,
            observation: AiObservationBinding {
                digest: strict.observation.digest,
            },
        };
        validate_transport_response(&request, response)
            .map_err(|error| invalid(format!("gateway response rejected: {error}")))
    }

    fn post_json(&self, path: &str, body: &[u8]) -> Result<Vec<u8>, AiProviderError> {
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
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(headers.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|error| unavailable(format!("writing gateway request: {error}")))?;

        let hard_limit = MAX_HEADER_BYTES
            .saturating_add(HEADER_TERMINATOR_BYTES)
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

pub fn new_gateway_request_id() -> String {
    next_request_id()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictGatewayResponse {
    contract_version: u32,
    request_id: String,
    proposal: AiCleanupProposal,
    observation: StrictObservationBinding,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictObservationBinding {
    digest: String,
}

fn parse_http_response(raw: &[u8], max_body_bytes: usize) -> Result<Vec<u8>, AiProviderError> {
    let boundary = raw
        .windows(HEADER_TERMINATOR_BYTES)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| invalid("gateway returned malformed HTTP headers"))?;
    if boundary > MAX_HEADER_BYTES {
        return Err(invalid("gateway response headers exceed size limit"));
    }

    let headers = std::str::from_utf8(&raw[..boundary])
        .map_err(|_| invalid("gateway returned non-UTF-8 HTTP headers"))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .ok_or_else(|| invalid("gateway response is missing HTTP status"))?;
    let mut status_parts = status.split_whitespace();
    let version = status_parts
        .next()
        .ok_or_else(|| invalid("gateway response has malformed HTTP status"))?;
    let code = status_parts
        .next()
        .ok_or_else(|| invalid("gateway response has malformed HTTP status"))?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || code != "200" {
        return Err(unavailable(format!(
            "gateway returned non-success status: {status}"
        )));
    }

    let mut content_length = None;
    let mut content_type = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(invalid("gateway returned malformed HTTP header"));
        };
        let name = name.trim();
        let value = value.trim();

        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid(
                "gateway Transfer-Encoding is unsupported; chunking is disabled",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| invalid("gateway returned invalid Content-Length"))?;
            if parsed > max_body_bytes {
                return Err(invalid(
                    "gateway response Content-Length exceeds configured size limit",
                ));
            }
            if content_length.replace(parsed).is_some() {
                return Err(invalid("gateway returned multiple Content-Length headers"));
            }
        }
        if name.eq_ignore_ascii_case("content-type") && content_type.replace(value).is_some() {
            return Err(invalid("gateway returned multiple Content-Type headers"));
        }
    }

    let Some(content_type) = content_type else {
        return Err(invalid("gateway response is missing Content-Type"));
    };
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err(invalid(
            "gateway response Content-Type must be application/json",
        ));
    }

    let body = &raw[boundary + HEADER_TERMINATOR_BYTES..];
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
    use crate::ai::{AiRecommendedAction, AiRisk};
    use crate::ai_contract::{AiDeterministicFacts, AiObservation, AiPathMode};
    use crate::ai_goal::{AiGoalCandidate, AiGoalRequest};
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

    fn goal_request() -> AiGoalRequest {
        AiGoalRequest::new(
            "req-goal".to_owned(),
            42,
            vec![AiGoalCandidate {
                candidate_id: 7,
                path: "<redacted>/.cache/pip".to_owned(),
                path_mode: AiPathMode::Redacted,
                size_bytes: 4096,
                risk: "low".to_owned(),
                rule_id: "cache-pip".to_owned(),
                category: "cache".to_owned(),
            }],
            2048,
            "low".to_owned(),
            None,
        )
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
    fn request_serialization_is_strict_json_with_exact_binding() {
        let analysis = analysis_request();
        let request = AiTransportRequest::new("req-1".to_owned(), analysis.observation.clone());
        let encoded = serde_json::to_vec(&request).expect("serialize request");
        let decoded: AiTransportRequest = serde_json::from_slice(&encoded).expect("parse request");
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
            let request = read_http_request(&mut stream);
            let transport: AiTransportRequest =
                serde_json::from_slice(&request).expect("request JSON");
            assert_eq!(transport.candidate.observation.digest, expected_digest);

            let request_id = transport.request_id.clone();
            let digest = transport.candidate.observation.digest.clone();
            let response = serde_json::json!({
                "contract_version": 1,
                "request_id": request_id,
                "proposal": {
                    "schema_version": 1,
                    "classification": "regenerable_cache",
                    "confidence": 0.91,
                    "rationale": ["generated cache"],
                    "caveats": [],
                    "risk": "medium",
                    "recommended_action": "review",
                    "provenance": {
                        "provider": "fake",
                        "model": "test",
                        "request_id": request_id,
                    }
                },
                "observation": { "digest": digest }
            });
            write_json_response(&mut stream, &serde_json::to_vec(&response).unwrap());
        });

        let provider = LoopbackGatewayProvider::new(LoopbackGatewayConfig {
            address,
            connect_timeout: Duration::from_secs(1),
            io_timeout: Duration::from_secs(1),
            max_request_bytes: 16 * 1024,
            max_response_bytes: 16 * 1024,
        });
        let proposal = provider.analyze(&analysis).expect("valid proposal");
        assert_eq!(proposal.classification, "regenerable_cache");
        assert_eq!(proposal.risk, AiRisk::Medium);
        assert_eq!(proposal.recommended_action, AiRecommendedAction::Review);
        server.join().unwrap();
    }

    #[test]
    fn local_goal_gateway_round_trip_uses_recommend_route_and_validates_selection() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
        let address = listener.local_addr().expect("local address");
        let request = goal_request();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let (path, body) = read_http_request_with_path(&mut stream);
            assert_eq!(path, RECOMMEND_PATH);
            let transport: AiGoalRequest = serde_json::from_slice(&body).expect("request JSON");
            let response = serde_json::json!({
                "contract_version": 1,
                "request_id": transport.request_id,
                "plan_digest": transport.plan_digest,
                "recommendation": {
                    "schema_version": 1,
                    "selected_candidate_ids": [7],
                    "rationale": ["largest supplied low-risk candidate"],
                    "caveats": [],
                    "provenance": {
                        "provider": "fake",
                        "model": "goal-test",
                        "request_id": transport.request_id
                    }
                }
            });
            write_json_response(&mut stream, &serde_json::to_vec(&response).unwrap());
        });

        let provider = LoopbackGatewayProvider::new(LoopbackGatewayConfig {
            address,
            connect_timeout: Duration::from_secs(1),
            io_timeout: Duration::from_secs(1),
            max_request_bytes: 16 * 1024,
            max_response_bytes: 16 * 1024,
        });
        let recommendation = provider.recommend_goal(&request).expect("valid recommendation");
        assert_eq!(recommendation.selected_candidate_ids, vec![7]);
        server.join().unwrap();
    }

    #[test]
    fn rejects_redirect_wrong_content_type_transfer_encoding_and_oversized_response() {
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/\r\nContent-Type: application/json\r\nContent-Length: 0\r\n\r\n";
        assert!(parse_http_response(redirect, 1024).is_err());

        let wrong_type =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{}";
        assert!(parse_http_response(wrong_type, 1024).is_err());

        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";
        assert!(parse_http_response(chunked, 1024).is_err());

        let oversized =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\n\r\ntest";
        assert!(parse_http_response(oversized, 3).is_err());
    }

    #[test]
    fn rejects_unknown_response_fields() {
        let response = br#"{
          "contract_version": 1,
          "request_id": "req",
          "proposal": {
            "schema_version": 1,
            "classification": "cache",
            "confidence": 0.9,
            "rationale": ["safe"],
            "caveats": [],
            "risk": "low",
            "recommended_action": "review",
            "provenance": {"provider":"fake","model":"test","request_id":"req"}
          },
          "observation": {"digest":"sha256:test"},
          "unexpected": true
        }"#;
        assert!(serde_json::from_slice::<StrictGatewayResponse>(response).is_err());
    }

    fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
        read_http_request_with_path(stream).1
    }

    fn read_http_request_with_path(stream: &mut TcpStream) -> (String, Vec<u8>) {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "request closed before body was complete");
            request.extend_from_slice(&buffer[..read]);
            if let Some(boundary) = request
                .windows(HEADER_TERMINATOR_BYTES)
                .position(|window| window == b"\r\n\r\n")
            {
                let headers = std::str::from_utf8(&request[..boundary]).unwrap();
                let mut lines = headers.lines();
                let request_line = lines.next().expect("request line");
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .expect("request path")
                    .to_owned();
                let length = lines
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap();
                if request.len() >= boundary + HEADER_TERMINATOR_BYTES + length {
                    let start = boundary + HEADER_TERMINATOR_BYTES;
                    return (path, request[start..start + length].to_vec());
                }
            }
        }
    }

    fn write_json_response(stream: &mut TcpStream, body: &[u8]) {
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }
}
