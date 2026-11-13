//! Credential redaction of diagnostic URLs and headers, a copy of the pure helpers in the diagnostics plugin
//! (a plugin cannot depend on another plugin, and the credential registry is the same single list).

const CREDENTIAL_NAMES: &[&str] = &[
    "apikey",
    "authorization",
    "proxyauthorization",
    "password",
    "secret",
    "clientsecret",
    "accesstoken",
    "refreshtoken",
    "idtoken",
    "authtoken",
    "token",
    "cookie",
    "setcookie",
    "xapikey",
    "xgoogapikey",
    "xauthtoken",
    "xaccesstoken",
];

/// True when `name` itself names a credential in any spelling (`api_key`, `api-key`, `apiKey`, `API key`).
fn is_credential_name(name: &str) -> bool {
    let normalized: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    CREDENTIAL_NAMES.contains(&normalized.as_str())
}

/// Removes URL user-info, every query value, and any fragment before an
/// endpoint enters a diagnostic event. Provider and proxy URLs are
/// user-configurable and may embed tokens in all three places even when the
/// normal built-in endpoints do not.
pub fn redacted_url(url: &str) -> String {
    // A fragment follows the query in a well-formed URL, so it has to come off
    // first -- and it has to come off at all, because a redirect URL carrying
    // `#access_token=...` has no `?` for the query branch below to catch.
    let (before_fragment, had_fragment) = match url.split_once('#') {
        Some((base, _fragment)) => (base, true),
        None => (url, false),
    };
    let (without_query, had_query) = match before_fragment.split_once('?') {
        Some((base, _query)) => (base, true),
        None => (before_fragment, false),
    };
    let mut redacted = redacted_user_info(without_query);
    if had_query {
        redacted.push_str("?<redacted>");
    }
    if had_fragment {
        redacted.push_str("#<redacted>");
    }
    redacted
}

/// Replaces a `user:password@` prefix on the authority of a URL whose query
/// and fragment have already been removed.
///
/// The authority is redacted whether or not a scheme is present. A
/// user-configured proxy is the one endpoint that routinely carries
/// credentials there, it is commonly written scheme-less
/// (`user:password@proxy.example:8080`), and a scheme-less proxy string is
/// also the shape most likely to be rejected and end up quoted verbatim in a
/// `GatewayError::InvalidProxy` message.
fn redacted_user_info(url_without_query: &str) -> String {
    let (scheme_prefix, authority_and_path) = match url_without_query.split_once("://") {
        Some((scheme, remainder)) => (format!("{scheme}://"), remainder),
        None => (String::new(), url_without_query),
    };
    // `/` is ASCII, so this byte index is always a character boundary and the
    // split below cannot land inside a multi-byte character.
    let authority_end = authority_and_path
        .find('/')
        .unwrap_or(authority_and_path.len());
    let (authority, path) = authority_and_path.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_user_info, host)) => format!("{scheme_prefix}<redacted>@{host}{path}"),
        None => url_without_query.to_owned(),
    }
}

/// Redacts credential-bearing HTTP header values while leaving request IDs,
/// rate-limit state, content metadata, and other debugging headers intact.
pub fn redacted_header_value(name: &str, value: &str) -> String {
    if is_credential_name(name) {
        "<redacted>".to_owned()
    } else {
        value.to_owned()
    }
}

/// Keeps only response headers that help diagnose payload shape, request
/// correlation, throttling, or retry behavior. Browser-security policy
/// headers can be many kilobytes long and add no value to an API-call log.
pub fn is_diagnostic_response_header(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "content-type"
            | "content-length"
            | "retry-after"
            | "request-id"
            | "x-request-id"
            | "openai-request-id"
            | "cf-ray"
    ) || normalized.starts_with("x-ratelimit-")
}
