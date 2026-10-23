//! The HTTP answer shape that a registry serves and a client reads (reference-registry.md).

/// One HTTP answer: status, content type, optional content encoding and body (PKG-818).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The status code.
    pub status_code: u16,
    /// The `Content-Type` header value.
    pub content_type_header: String,
    /// The `Content-Encoding` header value, if any.
    pub content_encoding: Option<String>,
    /// The body bytes as sent.
    pub body_bytes: Vec<u8>,
}

impl HttpResponse {
    /// An answer with the given parts (PKG-818).
    pub fn new(
        _status: u16,
        _content_type: &str,
        _content_encoding: Option<&str>,
        _body: Vec<u8>,
    ) -> HttpResponse {
        unimplemented!("api::http::HttpResponse::new")
    }

    /// An answer whose body is `decoded` compressed with gzip (PKG-818).
    pub fn with_gzip_body(_status: u16, _content_type: &str, _decoded: &[u8]) -> HttpResponse {
        unimplemented!("api::http::HttpResponse::with_gzip_body")
    }

    /// The status code.
    pub fn status(&self) -> u16 {
        unimplemented!("api::http::HttpResponse::status")
    }

    /// The content type.
    pub fn content_type(&self) -> &str {
        unimplemented!("api::http::HttpResponse::content_type")
    }

    /// The body bytes as sent.
    pub fn body(&self) -> &[u8] {
        unimplemented!("api::http::HttpResponse::body")
    }
}
