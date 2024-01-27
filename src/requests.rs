use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use hyper::{body::HttpBody, Body, Client, HeaderMap, Method, Request, Response};
use hyper_rustls::ConfigBuilderExt;
use lazy_static::lazy_static;
use rustls::client::ServerCertVerifier;
use std::{sync::Arc, time::SystemTime};
use tokio::sync::Mutex;

pub struct AuthInfo {
    pub cookie: String,
    pub student_id: u32,
    pub register_id: u32,
    pub school_year: u32,
}

impl Default for AuthInfo {
    fn default() -> Self {
        Self {
            cookie: "".to_owned(),
            student_id: Default::default(),
            register_id: Default::default(),
            school_year: 2022,
        }
    }
}

pub struct CalendarCache {
    pub last_updated: Option<DateTime<Local>>,
    pub regular_calendar: Option<String>,
    pub replacements_calendar: Option<String>,
}

impl CalendarCache {
    pub fn is_valid(&self) -> bool {
        if let Some(last_updated) = self.last_updated {
            (Local::now()
                .signed_duration_since(last_updated)
                .num_minutes()
                <= 5)
                && self.replacements_calendar.is_some()
                && self.regular_calendar.is_some()
        } else {
            false
        }
    }
}

impl Default for CalendarCache {
    fn default() -> Self {
        Self {
            last_updated: None,
            regular_calendar: None,
            replacements_calendar: None,
        }
    }
}

pub enum Group {
    One,
    Two,
}

const SERVER_IP: &'static str = "https://82.177.190.81";

lazy_static! {
    pub static ref GROUP_ONE_AUTH: Mutex<AuthInfo> = Mutex::new(AuthInfo {
        student_id: 4033,
        register_id: 1403,
        ..Default::default()
    });
    pub static ref GROUP_ONE_CACHE: Mutex<CalendarCache> = Mutex::new(CalendarCache::default());
    pub static ref GROUP_TWO_AUTH: Mutex<AuthInfo> = Mutex::new(AuthInfo {
        student_id: 4040,
        register_id: 1403,
        ..Default::default()
    });
    pub static ref GROUP_TWO_CACHE: Mutex<CalendarCache> = Mutex::new(CalendarCache::default());
}

pub enum Host {
    UonetPlus,
    UonetPlusUczen,
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Host::UonetPlus => "uonetplus.vulcan.net.pl",
            Host::UonetPlusUczen => "uonetplus-uczen.vulcan.net.pl",
        })
    }
}

pub struct InsecureVerifier {}

impl ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> std::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub async fn body_text(body: Body) -> Result<String> {
    String::from_utf8(hyper::body::to_bytes(body).await?.into_iter().collect())
        .context("Failed to convert body to string")
}

pub async fn post(
    relative_url: impl Into<String>,
    auth_info: &AuthInfo,
    host: Host,
    body: Option<impl Into<Body>>,
    headers: Option<HeaderMap>,
) -> Result<Response<Body>> {
    let body = match body {
        None => Body::empty(),
        Some(v) => v.into(),
    };

    let url = format!("{}{}", SERVER_IP, relative_url.into());

    let mut config = rustls::client::ClientConfig::builder()
        .with_safe_defaults()
        .with_native_roots()
        .with_no_client_auth();

    let v = Arc::new(InsecureVerifier {});
    config.dangerous().set_certificate_verifier(v);

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_only()
        .enable_http1()
        .build();

    let client: Client<_, Body> = Client::builder().build(https);

    let mut req = Request::builder()
        .method(Method::POST)
        .uri(url)
        .header("Host", host.to_string())
        .header(
            "Cookie",
            format!(
                "EfebSsoCookie={}; idBiezacyUczen={}; idBiezacyDziennik={}; biezacyRokSzkolny={}",
                auth_info.cookie,
                auth_info.student_id,
                auth_info.register_id,
                auth_info.school_year
                ))
        .header("Content-Length", body.size_hint().exact().unwrap_or(0));

    let req_headers = req.headers_mut().context("Failed to build request")?;

    if let Some(headers) = headers {
        req_headers.extend(headers);
    }

    client
        .request(req.body(body)?)
        .await
        .context("POST request failed")
}

pub async fn get(
    relative_url: impl Into<String>,
    auth_info: &AuthInfo,
    host: Host,
    headers: Option<HeaderMap>,
) -> Result<Response<Body>> {
    let url = format!("{}{}", SERVER_IP, relative_url.into());

    let mut config = rustls::client::ClientConfig::builder()
        .with_safe_defaults()
        .with_native_roots()
        .with_no_client_auth();

    let v = Arc::new(InsecureVerifier {});
    config.dangerous().set_certificate_verifier(v);

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_only()
        .enable_http1()
        .build();

    let client: Client<_, Body> = Client::builder().build(https);

    let mut req = Request::builder()
        .method(Method::GET)
        .uri(url)
        .header("Host", host.to_string())
        .header("Cookie", format!("EfebSsoCookie={}", auth_info.cookie));

    let req_headers = req.headers_mut().context("Failed to build request")?;

    if let Some(headers) = headers {
        req_headers.extend(headers);
    }

    let req = req.body(Body::empty())?;

    client.request(req).await.context("GET request failed")
}
