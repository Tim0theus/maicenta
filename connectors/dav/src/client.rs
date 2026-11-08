//! `WebDAV` client for `CalDAV` (RFC 4791) and `CardDAV` (RFC 6352): discovery
//! of collections, listing and fetching of resources, and conditional
//! `PUT`/`DELETE`.
//!
//! Servers answer `PROPFIND` and `REPORT` with `multistatus` XML. The
//! namespace-aware parser in [`xml`] is deliberately small: it understands
//! exactly the subset DAV servers produce.

use std::{
    fmt::{self, Write as _},
    time::Duration,
};

use maicenta_domain::CollectionKind;
use reqwest::{Method, StatusCode, header};
use url::Url;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_REDIRECTS: usize = 5;
const MULTIGET_BATCH: usize = 50;

const NS_DAV: &str = "DAV:";
const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
const NS_CALENDARSERVER: &str = "http://calendarserver.org/ns/";
const NS_APPLE: &str = "http://apple.com/ns/ical/";

/// Errors raised while talking to a DAV server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DavError {
    /// The URL is malformed or not allowed (only `https://`, or `http://`
    /// on the loopback interface for tests).
    InvalidUrl(String),
    /// The server could not be reached.
    Connection(String),
    /// The server rejected the credentials.
    Authentication,
    /// The resource does not exist.
    NotFound,
    /// The entity tag no longer matches: the server copy changed.
    PreconditionFailed,
    /// The server answered with an unexpected status.
    Server(u16, String),
    /// The server response could not be understood.
    Protocol(String),
}

impl fmt::Display for DavError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(detail) => write!(formatter, "invalid server URL: {detail}"),
            Self::Connection(detail) => write!(formatter, "connection failed: {detail}"),
            Self::Authentication => formatter.write_str("the server rejected the credentials"),
            Self::NotFound => formatter.write_str("resource not found"),
            Self::PreconditionFailed => formatter.write_str("the server copy changed meanwhile"),
            Self::Server(status, detail) => {
                write!(formatter, "server answered {status}: {detail}")
            }
            Self::Protocol(detail) => write!(formatter, "unexpected server response: {detail}"),
        }
    }
}

impl std::error::Error for DavError {}

/// A calendar, task list, or address book found during discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredCollection {
    pub kind: CollectionKind,
    /// Absolute path on the server.
    pub href: String,
    pub display_name: String,
    pub color: Option<String>,
    pub ctag: Option<String>,
}

/// One resource of a collection as listed by `PROPFIND`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteResource {
    pub href: String,
    pub etag: Option<String>,
}

/// One resource with its body as returned by a multiget `REPORT`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchedResource {
    pub href: String,
    pub etag: Option<String>,
    pub data: String,
}

/// Collection listing: the collection tag and its member resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionListing {
    pub ctag: Option<String>,
    pub resources: Vec<RemoteResource>,
}

/// Minimal namespace-aware XML document model.
pub mod xml {
    use super::DavError;

    /// One element with resolved namespace, attributes, concatenated text,
    /// and children.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct Element {
        pub namespace: String,
        pub name: String,
        /// Attributes other than namespace declarations, unescaped.
        pub attributes: Vec<(String, String)>,
        pub text: String,
        pub children: Vec<Element>,
    }

    impl Element {
        /// Value of an attribute by local name.
        #[must_use]
        pub fn attribute(&self, name: &str) -> Option<&str> {
            self.attributes
                .iter()
                .find(|(key, _)| {
                    key.eq_ignore_ascii_case(name) || key.ends_with(&format!(":{name}"))
                })
                .map(|(_, value)| value.as_str())
        }

        /// Whether the element has the given namespace and local name.
        #[must_use]
        pub fn is(&self, namespace: &str, name: &str) -> bool {
            self.namespace == namespace && self.name.eq_ignore_ascii_case(name)
        }

        /// First direct child with the given namespace and name.
        #[must_use]
        pub fn child(&self, namespace: &str, name: &str) -> Option<&Element> {
            self.children.iter().find(|child| child.is(namespace, name))
        }

        /// All direct children with the given namespace and name.
        pub fn children_named<'a>(
            &'a self,
            namespace: &'a str,
            name: &'a str,
        ) -> impl Iterator<Item = &'a Element> {
            self.children
                .iter()
                .filter(move |child| child.is(namespace, name))
        }

        /// First descendant (depth first) with the given namespace and name.
        #[must_use]
        pub fn descendant(&self, namespace: &str, name: &str) -> Option<&Element> {
            for child in &self.children {
                if child.is(namespace, name) {
                    return Some(child);
                }
                if let Some(found) = child.descendant(namespace, name) {
                    return Some(found);
                }
            }
            None
        }

        /// Text of the first descendant with the given name, trimmed.
        #[must_use]
        pub fn descendant_text(&self, namespace: &str, name: &str) -> Option<String> {
            self.descendant(namespace, name)
                .map(|element| element.text.trim().to_owned())
                .filter(|text| !text.is_empty())
        }
    }

    #[derive(Debug)]
    struct Frame {
        element: Element,
        prefixes: Vec<(String, String)>,
    }

    /// Parses an XML document into its root element.
    ///
    /// # Errors
    ///
    /// Returns [`DavError::Protocol`] for malformed XML.
    pub fn parse(text: &str) -> Result<Element, DavError> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut root = None;
        let mut rest = text;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix("<?") {
                let end = after.find("?>").ok_or_else(|| {
                    DavError::Protocol("unterminated processing instruction".into())
                })?;
                rest = &after[end + 2..];
            } else if let Some(after) = rest.strip_prefix("<!--") {
                let end = after
                    .find("-->")
                    .ok_or_else(|| DavError::Protocol("unterminated comment".into()))?;
                rest = &after[end + 3..];
            } else if let Some(after) = rest.strip_prefix("<![CDATA[") {
                let end = after
                    .find("]]>")
                    .ok_or_else(|| DavError::Protocol("unterminated CDATA".into()))?;
                if let Some(frame) = stack.last_mut() {
                    frame.element.text.push_str(&after[..end]);
                }
                rest = &after[end + 3..];
            } else if let Some(after) = rest.strip_prefix("<!") {
                let end = after
                    .find('>')
                    .ok_or_else(|| DavError::Protocol("unterminated declaration".into()))?;
                rest = &after[end + 1..];
            } else if let Some(after) = rest.strip_prefix("</") {
                let end = after
                    .find('>')
                    .ok_or_else(|| DavError::Protocol("unterminated end tag".into()))?;
                let frame = stack
                    .pop()
                    .ok_or_else(|| DavError::Protocol("end tag without start tag".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.element.children.push(frame.element),
                    None => root = Some(frame.element),
                }
                rest = &after[end + 1..];
            } else if let Some(after) = rest.strip_prefix('<') {
                let end = find_tag_end(after)
                    .ok_or_else(|| DavError::Protocol("unterminated start tag".into()))?;
                let tag = &after[..end];
                let (tag, self_closing) = match tag.strip_suffix('/') {
                    Some(stripped) => (stripped, true),
                    None => (tag, false),
                };
                let (qualified, attributes) = split_tag(tag);
                let mut prefixes = Vec::new();
                let mut plain_attributes = Vec::new();
                for (name, value) in attributes {
                    if name == "xmlns" {
                        prefixes.push((String::new(), value));
                    } else if let Some(prefix) = name.strip_prefix("xmlns:") {
                        prefixes.push((prefix.to_owned(), value));
                    } else {
                        plain_attributes.push((name, value));
                    }
                }
                let (prefix, local) = match qualified.split_once(':') {
                    Some((prefix, local)) => (prefix, local),
                    None => ("", qualified),
                };
                let namespace = resolve_prefix(&stack, &prefixes, prefix);
                let frame = Frame {
                    element: Element {
                        namespace,
                        name: local.to_owned(),
                        attributes: plain_attributes,
                        text: String::new(),
                        children: Vec::new(),
                    },
                    prefixes,
                };
                if self_closing {
                    match stack.last_mut() {
                        Some(parent) => parent.element.children.push(frame.element),
                        None => root = Some(frame.element),
                    }
                } else {
                    stack.push(frame);
                }
                rest = &after[end + 1..];
            } else {
                let end = rest.find('<').unwrap_or(rest.len());
                if let Some(frame) = stack.last_mut() {
                    frame.element.text.push_str(&unescape(&rest[..end]));
                }
                rest = &rest[end..];
            }
        }
        if !stack.is_empty() {
            return Err(DavError::Protocol(
                "document ended inside an element".into(),
            ));
        }
        root.ok_or_else(|| DavError::Protocol("empty document".into()))
    }

    fn find_tag_end(text: &str) -> Option<usize> {
        let mut quote: Option<char> = None;
        for (index, character) in text.char_indices() {
            match (quote, character) {
                (Some(open), c) if c == open => quote = None,
                (None, '"' | '\'') => quote = Some(character),
                (None, '>') => return Some(index),
                _ => {}
            }
        }
        None
    }

    fn split_tag(tag: &str) -> (&str, Vec<(String, String)>) {
        let tag = tag.trim();
        let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
        let name = &tag[..name_end];
        let mut attributes = Vec::new();
        let mut rest = tag[name_end..].trim_start();
        while !rest.is_empty() {
            let Some(equals) = rest.find('=') else { break };
            let attribute_name = rest[..equals].trim().to_owned();
            let after = rest[equals + 1..].trim_start();
            let Some(quote) = after.chars().next() else {
                break;
            };
            if quote != '"' && quote != '\'' {
                break;
            }
            let Some(close) = after[1..].find(quote) else {
                break;
            };
            attributes.push((attribute_name, unescape(&after[1..=close])));
            rest = after[close + 2..].trim_start();
        }
        (name, attributes)
    }

    fn resolve_prefix(stack: &[Frame], own: &[(String, String)], prefix: &str) -> String {
        if let Some((_, namespace)) = own.iter().rev().find(|(name, _)| name == prefix) {
            return namespace.clone();
        }
        for frame in stack.iter().rev() {
            if let Some((_, namespace)) =
                frame.prefixes.iter().rev().find(|(name, _)| name == prefix)
            {
                return namespace.clone();
            }
        }
        String::new()
    }

    fn unescape(text: &str) -> String {
        if !text.contains('&') {
            return text.to_owned();
        }
        let mut output = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(index) = rest.find('&') {
            output.push_str(&rest[..index]);
            let after = &rest[index + 1..];
            let Some(end) = after.find(';') else {
                output.push('&');
                rest = after;
                continue;
            };
            let entity = &after[..end];
            let replacement = match entity {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix('#')
                    .and_then(|number| {
                        number.strip_prefix('x').map_or_else(
                            || number.parse::<u32>().ok(),
                            |hex| u32::from_str_radix(hex, 16).ok(),
                        )
                    })
                    .and_then(char::from_u32),
            };
            if let Some(character) = replacement {
                output.push(character);
            } else {
                output.push('&');
                output.push_str(entity);
                output.push(';');
            }
            rest = &after[end + 1..];
        }
        output.push_str(rest);
        output
    }

    /// Escapes text for use inside an XML element.
    #[must_use]
    pub fn escape(text: &str) -> String {
        let mut output = String::with_capacity(text.len());
        for character in text.chars() {
            match character {
                '<' => output.push_str("&lt;"),
                '>' => output.push_str("&gt;"),
                '&' => output.push_str("&amp;"),
                '"' => output.push_str("&quot;"),
                other => output.push(other),
            }
        }
        output
    }
}

/// Authenticated client bound to one server.
#[derive(Clone)]
pub struct DavClient {
    http: reqwest::Client,
    base: Url,
    username: String,
    password: String,
}

impl fmt::Debug for DavClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DavClient")
            .field("base", &self.base.as_str())
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// Parsed `multistatus` response entry.
#[derive(Clone, Debug)]
struct ResponseEntry {
    href: String,
    /// Properties of the successful `propstat`, keyed by namespace and name.
    props: xml::Element,
    status_ok: bool,
}

impl DavClient {
    /// Creates a client. Only `https://` servers are accepted, plus plain
    /// `http://` on the loopback interface for local test servers.
    ///
    /// # Errors
    ///
    /// Returns [`DavError::InvalidUrl`] for unsupported URLs.
    pub fn new(server_url: &str, username: &str, password: &str) -> Result<Self, DavError> {
        let trimmed = server_url.trim();
        let with_scheme = if trimmed.contains("://") {
            trimmed.to_owned()
        } else {
            format!("https://{trimmed}")
        };
        let base =
            Url::parse(&with_scheme).map_err(|error| DavError::InvalidUrl(error.to_string()))?;
        let loopback = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
        if base.scheme() != "https" && !(base.scheme() == "http" && loopback) {
            return Err(DavError::InvalidUrl(
                "only https:// servers are supported".into(),
            ));
        }
        if username.trim().is_empty() {
            return Err(DavError::InvalidUrl("a user name is required".into()));
        }
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| DavError::Connection(error.to_string()))?;
        Ok(Self {
            http,
            base,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    /// The server root used to resolve hrefs.
    #[must_use]
    pub fn base(&self) -> &Url {
        &self.base
    }

    /// Resolves an absolute path or full URL against the server.
    ///
    /// # Errors
    ///
    /// Returns [`DavError::InvalidUrl`] for unresolvable hrefs.
    pub fn resolve(&self, href: &str) -> Result<Url, DavError> {
        self.base
            .join(href)
            .map_err(|error| DavError::InvalidUrl(format!("{href}: {error}")))
    }

    async fn send(
        &self,
        method: &str,
        url: Url,
        headers: &[(&str, String)],
        body: Option<String>,
    ) -> Result<reqwest::Response, DavError> {
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|error| DavError::Protocol(error.to_string()))?;
        let mut url = url;
        for _ in 0..=MAX_REDIRECTS {
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .basic_auth(&self.username, Some(&self.password));
            for (name, value) in headers {
                request = request.header(*name, value);
            }
            if let Some(body) = &body {
                request = request.body(body.clone());
            }
            let response = request
                .send()
                .await
                .map_err(|error| DavError::Connection(error.to_string()))?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| DavError::Protocol("redirect without location".into()))?;
                url = url
                    .join(location)
                    .map_err(|error| DavError::InvalidUrl(error.to_string()))?;
                continue;
            }
            return match status {
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(DavError::Authentication),
                StatusCode::NOT_FOUND => Err(DavError::NotFound),
                StatusCode::PRECONDITION_FAILED => Err(DavError::PreconditionFailed),
                status if status.is_success() => Ok(response),
                status => {
                    let detail = response.text().await.unwrap_or_default();
                    Err(DavError::Server(
                        status.as_u16(),
                        detail.chars().take(300).collect(),
                    ))
                }
            };
        }
        Err(DavError::Protocol("too many redirects".into()))
    }

    async fn multistatus(
        &self,
        method: &str,
        url: Url,
        depth: &str,
        body: String,
    ) -> Result<Vec<ResponseEntry>, DavError> {
        let response = self
            .send(
                method,
                url,
                &[
                    ("Depth", depth.to_owned()),
                    ("Content-Type", "application/xml; charset=utf-8".to_owned()),
                ],
                Some(body),
            )
            .await?;
        let text = response
            .text()
            .await
            .map_err(|error| DavError::Connection(error.to_string()))?;
        let document = xml::parse(&text)?;
        if !document.is(NS_DAV, "multistatus") {
            return Err(DavError::Protocol(format!(
                "expected multistatus, got {}",
                document.name
            )));
        }
        let mut entries = Vec::new();
        for response in document.children_named(NS_DAV, "response") {
            let Some(href) = response.descendant_text(NS_DAV, "href") else {
                continue;
            };
            let mut props = xml::Element::default();
            let mut status_ok = false;
            for propstat in response.children_named(NS_DAV, "propstat") {
                let status = propstat
                    .descendant_text(NS_DAV, "status")
                    .unwrap_or_default();
                let ok = status.contains(" 200 ") || status.ends_with(" 200");
                if let Some(prop) = propstat.child(NS_DAV, "prop") {
                    if ok {
                        status_ok = true;
                        props.children.extend(prop.children.iter().cloned());
                    }
                }
            }
            // A response without propstat (REPORT errors) still carries a
            // status we treat as "not available".
            entries.push(ResponseEntry {
                href: normalize_href(&href),
                props,
                status_ok,
            });
        }
        Ok(entries)
    }

    /// Finds every calendar and address book the account can use.
    ///
    /// # Errors
    ///
    /// Returns a [`DavError`] when the server cannot be reached, rejects
    /// the credentials, or exposes no collections.
    pub async fn discover(&self) -> Result<Vec<DiscoveredCollection>, DavError> {
        let mut homes: Vec<(String, bool)> = Vec::new(); // (href, is_calendar_home)
        let base_path = self.base.path().to_owned();
        let starting_points = {
            let mut points = vec![base_path.clone()];
            if !base_path.trim_end_matches('/').is_empty() {
                points.push("/".to_owned());
            }
            points.push("/.well-known/caldav".to_owned());
            points.push("/.well-known/carddav".to_owned());
            points
        };
        let mut principal = None;
        let mut last_error = None;
        for point in &starting_points {
            match self.read_homes(point).await {
                Ok((found_principal, found_homes)) => {
                    homes.extend(found_homes);
                    if principal.is_none() {
                        principal = found_principal;
                    }
                    if !homes.is_empty() {
                        break;
                    }
                }
                Err(DavError::Authentication) => return Err(DavError::Authentication),
                Err(error) => last_error = Some(error),
            }
            if let Some(principal_href) = principal.clone() {
                if let Ok((_, found)) = self.read_homes(&principal_href).await {
                    homes.extend(found);
                    if !homes.is_empty() {
                        break;
                    }
                }
            }
        }
        if homes.is_empty() {
            // Some servers (Radicale) list collections directly under the
            // user's path; treat the base as both home sets.
            homes.push((base_path.clone(), true));
            homes.push((base_path, false));
        }
        homes.sort();
        homes.dedup();
        let mut collections = Vec::new();
        for (home, _) in &homes {
            match self.list_collections(home).await {
                Ok(found) => collections.extend(found),
                Err(DavError::Authentication) => return Err(DavError::Authentication),
                Err(error) => last_error = Some(error),
            }
        }
        collections.sort_by(|a, b| a.href.cmp(&b.href));
        collections.dedup_by(|a, b| a.href == b.href);
        if collections.is_empty() {
            return Err(last_error.unwrap_or_else(|| {
                DavError::Protocol("no calendars or address books found".into())
            }));
        }
        Ok(collections)
    }

    /// Reads `current-user-principal` and the home sets from one path.
    async fn read_homes(
        &self,
        path: &str,
    ) -> Result<(Option<String>, Vec<(String, bool)>), DavError> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:C="{NS_CALDAV}" xmlns:CARD="{NS_CARDDAV}">
  <D:prop>
    <D:current-user-principal/>
    <D:resourcetype/>
    <C:calendar-home-set/>
    <CARD:addressbook-home-set/>
  </D:prop>
</D:propfind>"#
        );
        let entries = self
            .multistatus("PROPFIND", self.resolve(path)?, "0", body)
            .await?;
        let mut principal = None;
        let mut homes = Vec::new();
        for entry in entries {
            if let Some(element) = entry.props.child(NS_DAV, "current-user-principal") {
                if let Some(href) = element.descendant_text(NS_DAV, "href") {
                    principal = Some(normalize_href(&href));
                }
            }
            if let Some(element) = entry.props.child(NS_CALDAV, "calendar-home-set") {
                for href in element.children_named(NS_DAV, "href") {
                    homes.push((normalize_href(href.text.trim()), true));
                }
            }
            if let Some(element) = entry.props.child(NS_CARDDAV, "addressbook-home-set") {
                for href in element.children_named(NS_DAV, "href") {
                    homes.push((normalize_href(href.text.trim()), false));
                }
            }
        }
        Ok((principal, homes))
    }

    /// Lists the collections directly inside a home set.
    async fn list_collections(&self, home: &str) -> Result<Vec<DiscoveredCollection>, DavError> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:C="{NS_CALDAV}" xmlns:CS="{NS_CALENDARSERVER}" xmlns:A="{NS_APPLE}">
  <D:prop>
    <D:resourcetype/>
    <D:displayname/>
    <CS:getctag/>
    <A:calendar-color/>
    <C:supported-calendar-component-set/>
  </D:prop>
</D:propfind>"#
        );
        let entries = self
            .multistatus("PROPFIND", self.resolve(home)?, "1", body)
            .await?;
        let mut collections = Vec::new();
        for entry in entries {
            if !entry.status_ok {
                continue;
            }
            let Some(resource_type) = entry.props.child(NS_DAV, "resourcetype") else {
                continue;
            };
            let is_calendar = resource_type.child(NS_CALDAV, "calendar").is_some();
            let is_address_book = resource_type.child(NS_CARDDAV, "addressbook").is_some();
            if !is_calendar && !is_address_book {
                continue;
            }
            let kind = if is_address_book {
                CollectionKind::AddressBook
            } else {
                // A calendar that only accepts VTODO is a task list; one
                // without an advertised component set is a full calendar.
                let names = component_names(&entry.props);
                if !names.is_empty()
                    && !names.iter().any(|name| name == "VEVENT")
                    && names.iter().any(|name| name == "VTODO")
                {
                    CollectionKind::TaskList
                } else {
                    CollectionKind::Calendar
                }
            };
            let display_name = entry
                .props
                .descendant_text(NS_DAV, "displayname")
                .unwrap_or_else(|| {
                    entry
                        .href
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("Kalender")
                        .to_owned()
                });
            collections.push(DiscoveredCollection {
                kind,
                href: entry.href,
                display_name,
                color: entry
                    .props
                    .descendant_text(NS_APPLE, "calendar-color")
                    .map(|color| color.chars().take(7).collect()),
                ctag: entry.props.descendant_text(NS_CALENDARSERVER, "getctag"),
            });
        }
        Ok(collections)
    }

    /// Lists the resources of a collection with their entity tags.
    ///
    /// # Errors
    ///
    /// Returns a [`DavError`] when the listing fails.
    pub async fn list_resources(
        &self,
        collection_href: &str,
    ) -> Result<CollectionListing, DavError> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:" xmlns:CS="{NS_CALENDARSERVER}">
  <D:prop>
    <D:resourcetype/>
    <D:getetag/>
    <CS:getctag/>
  </D:prop>
</D:propfind>"#
        );
        let entries = self
            .multistatus("PROPFIND", self.resolve(collection_href)?, "1", body)
            .await?;
        let collection_path = normalize_href(collection_href);
        let mut ctag = None;
        let mut resources = Vec::new();
        for entry in entries {
            let is_collection = entry
                .props
                .child(NS_DAV, "resourcetype")
                .is_some_and(|kind| kind.child(NS_DAV, "collection").is_some());
            if entry.href == collection_path || is_collection {
                if ctag.is_none() {
                    ctag = entry.props.descendant_text(NS_CALENDARSERVER, "getctag");
                }
                continue;
            }
            resources.push(RemoteResource {
                etag: entry.props.descendant_text(NS_DAV, "getetag"),
                href: entry.href,
            });
        }
        Ok(CollectionListing { ctag, resources })
    }

    /// Fetches the bodies of the given resources through a multiget report.
    ///
    /// # Errors
    ///
    /// Returns a [`DavError`] when the report fails.
    pub async fn fetch_resources(
        &self,
        collection_href: &str,
        kind: CollectionKind,
        hrefs: &[String],
    ) -> Result<Vec<FetchedResource>, DavError> {
        let mut fetched = Vec::new();
        for batch in hrefs.chunks(MULTIGET_BATCH) {
            let mut href_elements = String::new();
            for href in batch {
                let _ = writeln!(href_elements, "  <D:href>{}</D:href>", xml::escape(href));
            }
            let (body, data_namespace, data_name) = match kind {
                CollectionKind::AddressBook => (
                    format!(
                        r#"<?xml version="1.0" encoding="utf-8"?>
<CARD:addressbook-multiget xmlns:D="DAV:" xmlns:CARD="{NS_CARDDAV}">
  <D:prop><D:getetag/><CARD:address-data/></D:prop>
{href_elements}</CARD:addressbook-multiget>"#
                    ),
                    NS_CARDDAV,
                    "address-data",
                ),
                CollectionKind::Calendar | CollectionKind::TaskList => (
                    format!(
                        r#"<?xml version="1.0" encoding="utf-8"?>
<C:calendar-multiget xmlns:D="DAV:" xmlns:C="{NS_CALDAV}">
  <D:prop><D:getetag/><C:calendar-data/></D:prop>
{href_elements}</C:calendar-multiget>"#
                    ),
                    NS_CALDAV,
                    "calendar-data",
                ),
            };
            let entries = self
                .multistatus("REPORT", self.resolve(collection_href)?, "1", body)
                .await?;
            for entry in entries {
                if !entry.status_ok {
                    continue;
                }
                let Some(data) = entry.props.descendant_text(data_namespace, data_name) else {
                    continue;
                };
                fetched.push(FetchedResource {
                    etag: entry.props.descendant_text(NS_DAV, "getetag"),
                    href: entry.href,
                    data,
                });
            }
        }
        Ok(fetched)
    }

    /// Stores a resource. With an entity tag the write only succeeds while
    /// the server copy is unchanged; without one the resource must not
    /// exist yet. Returns the new entity tag when the server reports one.
    ///
    /// # Errors
    ///
    /// Returns [`DavError::PreconditionFailed`] on a conflict or another
    /// [`DavError`] when the write fails.
    pub async fn put(
        &self,
        href: &str,
        body: String,
        content_type: &str,
        etag: Option<&str>,
    ) -> Result<Option<String>, DavError> {
        let mut headers = vec![("Content-Type", content_type.to_owned())];
        match etag {
            Some(etag) => headers.push(("If-Match", etag.to_owned())),
            None => headers.push(("If-None-Match", "*".to_owned())),
        }
        let response = self
            .send("PUT", self.resolve(href)?, &headers, Some(body))
            .await?;
        Ok(response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned))
    }

    /// Reads the current entity tag of one resource.
    ///
    /// # Errors
    ///
    /// Returns a [`DavError`] when the resource cannot be read.
    pub async fn etag(&self, href: &str) -> Result<Option<String>, DavError> {
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:"><D:prop><D:getetag/></D:prop></D:propfind>"#
            .to_owned();
        let entries = self
            .multistatus("PROPFIND", self.resolve(href)?, "0", body)
            .await?;
        Ok(entries
            .into_iter()
            .find_map(|entry| entry.props.descendant_text(NS_DAV, "getetag")))
    }

    /// Deletes a resource. A missing resource counts as deleted.
    ///
    /// # Errors
    ///
    /// Returns [`DavError::PreconditionFailed`] when the entity tag no
    /// longer matches or another [`DavError`] when the deletion fails.
    pub async fn delete(&self, href: &str, etag: Option<&str>) -> Result<(), DavError> {
        let mut headers = Vec::new();
        if let Some(etag) = etag {
            headers.push(("If-Match", etag.to_owned()));
        }
        match self
            .send("DELETE", self.resolve(href)?, &headers, None)
            .await
        {
            Ok(_) | Err(DavError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Creates a calendar or address book collection (used by tests and
    /// first-run setups on servers without a default collection).
    ///
    /// # Errors
    ///
    /// Returns a [`DavError`] when the server refuses.
    pub async fn create_collection(
        &self,
        href: &str,
        kind: CollectionKind,
        display_name: &str,
    ) -> Result<(), DavError> {
        let name = xml::escape(display_name);
        let (method, body) = match kind {
            CollectionKind::AddressBook => (
                "MKCOL",
                format!(
                    r#"<?xml version="1.0" encoding="utf-8"?>
<D:mkcol xmlns:D="DAV:" xmlns:CARD="{NS_CARDDAV}">
  <D:set><D:prop>
    <D:resourcetype><D:collection/><CARD:addressbook/></D:resourcetype>
    <D:displayname>{name}</D:displayname>
  </D:prop></D:set>
</D:mkcol>"#
                ),
            ),
            CollectionKind::Calendar | CollectionKind::TaskList => (
                "MKCALENDAR",
                format!(
                    r#"<?xml version="1.0" encoding="utf-8"?>
<C:mkcalendar xmlns:D="DAV:" xmlns:C="{NS_CALDAV}">
  <D:set><D:prop>
    <D:displayname>{name}</D:displayname>
    <C:supported-calendar-component-set>
      <C:comp name="VEVENT"/><C:comp name="VTODO"/>
    </C:supported-calendar-component-set>
  </D:prop></D:set>
</C:mkcalendar>"#
                ),
            ),
        };
        self.send(
            method,
            self.resolve(href)?,
            &[("Content-Type", "application/xml; charset=utf-8".to_owned())],
            Some(body),
        )
        .await
        .map(|_| ())
    }
}

/// Component names of a `supported-calendar-component-set`, read from the
/// `name` attribute of each `comp` element.
fn component_names(props: &xml::Element) -> Vec<String> {
    props
        .child(NS_CALDAV, "supported-calendar-component-set")
        .map(|set| {
            set.children_named(NS_CALDAV, "comp")
                .filter_map(|comp| comp.attribute("name"))
                .map(|name| name.trim().to_ascii_uppercase())
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Normalizes a response href to an absolute, percent-decoded path.
fn normalize_href(href: &str) -> String {
    let href = href.trim();
    let path = Url::parse(href).map_or_else(|_| href.to_owned(), |url| url.path().to_owned());
    percent_decode(&path)
}

fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&path[index + 1..index + 3], 16) {
                output.push(value);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).unwrap_or_else(|_| path.to_owned())
}

/// Percent-encodes a path segment for use in an href.
#[must_use]
pub fn encode_segment(segment: &str) -> String {
    let mut output = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'@' => {
                output.push(byte as char);
            }
            other => {
                let _ = write!(output, "%{other:02X}");
            }
        }
    }
    output
}

/// Stable 64-bit FNV-1a hash as 16 hex digits, used to derive local
/// identifiers from server paths.
#[must_use]
pub fn stable_key(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use maicenta_domain::CollectionKind;

    use super::{DavClient, DavError, encode_segment, normalize_href, stable_key, xml};

    #[test]
    fn parses_namespaced_multistatus() {
        let text = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/dav/calendars/timm/personal/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><C:calendar/></d:resourcetype>
        <d:displayname>Pers&#246;nlich &amp; Arbeit</d:displayname>
        <cs:getctag>"abc"</cs:getctag>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;
        let document = xml::parse(text).expect("parse");
        assert!(document.is("DAV:", "multistatus"));
        let response = document.child("DAV:", "response").expect("response");
        assert_eq!(
            response.descendant_text("DAV:", "href").as_deref(),
            Some("/dav/calendars/timm/personal/")
        );
        assert_eq!(
            response.descendant_text("DAV:", "displayname").as_deref(),
            Some("Persönlich & Arbeit")
        );
        let resource_type = response
            .descendant("DAV:", "resourcetype")
            .expect("resourcetype");
        assert!(
            resource_type
                .child("urn:ietf:params:xml:ns:caldav", "calendar")
                .is_some()
        );
        assert_eq!(
            response
                .descendant_text("http://calendarserver.org/ns/", "getctag")
                .as_deref(),
            Some("\"abc\"")
        );
        let set = xml::parse(
            r#"<C:supported-calendar-component-set xmlns:C="urn:ietf:params:xml:ns:caldav"><C:comp name="VEVENT"/><C:comp name='VTODO' /></C:supported-calendar-component-set>"#,
        )
        .expect("set");
        let names: Vec<_> = set
            .children_named("urn:ietf:params:xml:ns:caldav", "comp")
            .filter_map(|comp| comp.attribute("name"))
            .collect();
        assert_eq!(names, vec!["VEVENT", "VTODO"]);
    }

    #[test]
    fn validates_server_urls() {
        assert!(DavClient::new("https://cloud.example.org/remote.php/dav/", "timm", "pw").is_ok());
        assert!(DavClient::new("cloud.example.org", "timm", "pw").is_ok());
        assert!(DavClient::new("http://127.0.0.1:5232/", "timm", "pw").is_ok());
        assert_eq!(
            DavClient::new("http://cloud.example.org/", "timm", "pw").err(),
            Some(DavError::InvalidUrl(
                "only https:// servers are supported".into()
            ))
        );
        assert!(DavClient::new("https://cloud.example.org/", " ", "pw").is_err());
    }

    #[test]
    fn normalizes_hrefs_and_keys() {
        assert_eq!(
            normalize_href("https://cloud.example.org/dav/a%20b/"),
            "/dav/a b/"
        );
        assert_eq!(normalize_href("/dav/x.ics"), "/dav/x.ics");
        assert_eq!(encode_segment("a b@c.ics"), "a%20b@c.ics");
        assert_eq!(stable_key("x").len(), 16);
        assert_ne!(stable_key("x"), stable_key("y"));
        let _ = CollectionKind::Calendar;
    }
}
