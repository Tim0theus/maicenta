//! Standards-oriented MIME parsing and safe HTML preparation for the viewer.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::Arc,
    sync::atomic::{AtomicUsize, Ordering},
};

use ammonia::{Builder, UrlRelative};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use mail_parser::{Address, MessageParser, MimeHeaders, PartType};
use percent_encoding::percent_decode_str;

const MAX_CACHED_ATTACHMENTS: usize = 20;
const MAX_CACHED_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
const MAX_INLINE_IMAGES: usize = 20;
const MAX_INLINE_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_INLINE_IMAGE_TOTAL_BYTES: usize = 5 * 1024 * 1024;
const MAX_INLINE_IMAGE_DIMENSION: u32 = 4096;
const MAX_INLINE_IMAGE_PIXELS: u64 = 16_777_216;

/// Policy controlling how a message may load content from the network.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderPolicy {
    /// Permit HTTP(S) image sources. This should only be enabled by an explicit
    /// user action because image requests can disclose that a message was read.
    pub allow_remote_images: bool,
}

/// Content prepared for an isolated HTML viewer and a plain-text fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedMessage {
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub from_display_name: Option<String>,
    pub to_recipients: String,
    pub cc_recipients: String,
    pub bcc_recipients: String,
    /// Date header as a Unix timestamp in milliseconds.
    pub date_ms: Option<i64>,
    pub plain_text: Option<String>,
    pub sanitized_html: Option<String>,
    pub blocked_remote_images: usize,
    pub attachment_count: usize,
    /// Decoded, non-inline MIME attachments when the complete set is within
    /// the local cache limits. An empty vector with a non-zero count and
    /// `attachments_complete == false` means the server copy remains the
    /// source of truth because the set exceeded those limits.
    pub attachments: Vec<RenderedAttachment>,
    pub attachments_complete: bool,
}

/// One decoded MIME attachment safe to hand to the profile object store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedAttachment {
    pub file_name: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderError {
    InvalidMessage,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the input is not a parseable RFC 5322 message")
    }
}

impl std::error::Error for RenderError {}

/// Failure while decoding a separately fetched MIME attachment section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttachmentDecodeError {
    InvalidEncoding,
    UnsupportedEncoding,
    SizeLimitExceeded,
}

impl std::fmt::Display for AttachmentDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncoding => formatter.write_str("attachment transfer encoding is invalid"),
            Self::UnsupportedEncoding => {
                formatter.write_str("attachment transfer encoding is unsupported")
            }
            Self::SizeLimitExceeded => formatter.write_str("decoded attachment exceeds the limit"),
        }
    }
}

impl std::error::Error for AttachmentDecodeError {}

/// Decodes one raw MIME body section while enforcing the decoded byte limit.
///
/// This is intentionally separate from HTML rendering: IMAP `BODY.PEEK`
/// returns the body section with its transfer encoding still applied.
///
/// # Errors
///
/// Returns an error for malformed base64/quoted-printable input, unknown
/// transfer encodings, or an attachment larger than `max_decoded_bytes`.
pub fn decode_attachment_part(
    encoded: &[u8],
    transfer_encoding: &str,
    max_decoded_bytes: usize,
) -> Result<Vec<u8>, AttachmentDecodeError> {
    let decoded = match transfer_encoding.trim().to_ascii_lowercase().as_str() {
        "base64" => {
            let compact = encoded
                .iter()
                .copied()
                .filter(|byte| !byte.is_ascii_whitespace())
                .collect::<Vec<_>>();
            BASE64_STANDARD
                .decode(compact)
                .map_err(|_| AttachmentDecodeError::InvalidEncoding)?
        }
        "quoted-printable" => {
            mail_parser::decoders::quoted_printable::quoted_printable_decode(encoded)
                .ok_or(AttachmentDecodeError::InvalidEncoding)?
        }
        "7bit" | "8bit" | "binary" => encoded.to_vec(),
        _ => return Err(AttachmentDecodeError::UnsupportedEncoding),
    };
    if decoded.len() > max_decoded_bytes {
        return Err(AttachmentDecodeError::SizeLimitExceeded);
    }
    Ok(decoded)
}

/// Parses MIME messages and sanitizes their HTML independently of the UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct MessageRenderer;

impl MessageRenderer {
    /// Render a raw RFC 5322/MIME message according to `policy`.
    ///
    /// HTML is parsed using HTML5 rules. Scripts, event handlers, forms,
    /// embedded frames and unsafe URL schemes are removed. Remote images are
    /// blocked by default. Bounded `cid:` raster sources are resolved only
    /// against validated MIME inline parts from the same message and become
    /// in-memory data images for the isolated viewer.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidMessage`] when the input cannot be parsed
    /// as an RFC 5322 message.
    pub fn render(
        &self,
        raw_message: &[u8],
        policy: RenderPolicy,
    ) -> Result<RenderedMessage, RenderError> {
        let message = MessageParser::default()
            .parse(raw_message)
            .ok_or(RenderError::InvalidMessage)?;

        let blocked = Arc::new(AtomicUsize::new(0));
        let from = message.from().and_then(|address| address.first());
        let inline_images = Arc::new(extract_inline_image_data_uris(&message));
        let sanitized_html = message.body_html(0).map(|html| {
            sanitize_html(
                html.as_ref(),
                policy,
                Arc::clone(&blocked),
                Arc::clone(&inline_images),
            )
        });
        let (attachment_count, attachments, attachments_complete) =
            extract_downloadable_attachments(&message);

        Ok(RenderedMessage {
            subject: message.subject().map(str::to_owned),
            from_address: from
                .and_then(|address| address.address())
                .map(str::to_owned),
            from_display_name: from.and_then(|address| address.name()).map(str::to_owned),
            to_recipients: searchable_addresses(message.to()),
            cc_recipients: searchable_addresses(message.cc()),
            bcc_recipients: searchable_addresses(message.bcc()),
            date_ms: message
                .date()
                .map(|date| date.to_timestamp().saturating_mul(1000)),
            plain_text: message.body_text(0).map(Cow::into_owned),
            sanitized_html,
            blocked_remote_images: blocked.load(Ordering::Relaxed),
            attachment_count,
            attachments,
            attachments_complete,
        })
    }
}

fn searchable_addresses(addresses: Option<&Address<'_>>) -> String {
    addresses.map_or_else(String::new, |addresses| {
        addresses
            .iter()
            .take(500)
            .filter_map(
                |address| match (address.name.as_deref(), address.address.as_deref()) {
                    (Some(name), Some(email)) if !name.is_empty() => {
                        Some(format!("{name} <{email}>"))
                    }
                    (_, Some(email)) => Some(email.to_owned()),
                    (Some(name), None) if !name.is_empty() => Some(name.to_owned()),
                    _ => None,
                },
            )
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn extract_downloadable_attachments(
    message: &mail_parser::Message<'_>,
) -> (usize, Vec<RenderedAttachment>, bool) {
    let mut attachment_count = 0_usize;
    let mut total_bytes = 0_usize;
    let mut attachments = Vec::new();
    let mut within_limits = true;

    for part in message.attachments() {
        // Inline binary MIME resources belong to cid: rendering. A part with
        // an explicit attachment disposition is classified as Binary by
        // mail-parser and therefore remains downloadable here.
        if is_inline_mime_resource(part) {
            continue;
        }
        attachment_count += 1;
        let size = part.contents().len();
        if attachment_count > MAX_CACHED_ATTACHMENTS
            || size > MAX_CACHED_ATTACHMENT_BYTES.saturating_sub(total_bytes)
        {
            within_limits = false;
            continue;
        }
        total_bytes += size;
        let content_type = normalized_attachment_content_type(part);
        let file_name =
            safe_attachment_file_name(part.attachment_name(), attachment_count, &content_type);
        attachments.push(RenderedAttachment {
            file_name,
            content_type,
            body: part.contents().to_vec(),
        });
    }

    if within_limits {
        (attachment_count, attachments, true)
    } else {
        // Cache attachment sets atomically. This avoids presenting a partial
        // list without a way for the interface to explain what is missing.
        (attachment_count, Vec::new(), false)
    }
}

fn normalized_attachment_content_type(part: &mail_parser::MessagePart<'_>) -> String {
    part.content_type()
        .and_then(|content_type| {
            let subtype = content_type.c_subtype.as_deref()?;
            if valid_mime_token(&content_type.c_type) && valid_mime_token(subtype) {
                Some(format!(
                    "{}/{}",
                    content_type.c_type.to_ascii_lowercase(),
                    subtype.to_ascii_lowercase()
                ))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "application/octet-stream".into())
}

fn valid_mime_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        })
}

/// Converts sender-controlled MIME names into a portable display/save name.
#[must_use]
pub fn safe_attachment_file_name(
    supplied: Option<&str>,
    position: usize,
    content_type: &str,
) -> String {
    let supplied = supplied
        .and_then(|name| name.rsplit(['/', '\\']).next())
        .unwrap_or_default();
    let sanitized = supplied
        .chars()
        .take(180)
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim_matches(|character| character == ' ' || character == '.');
    if !sanitized.is_empty() {
        return sanitized.to_owned();
    }

    let extension = match content_type {
        "application/pdf" => "pdf",
        "message/rfc822" => "eml",
        "text/calendar" => "ics",
        "text/csv" => "csv",
        "text/html" => "html",
        "text/plain" => "txt",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "application/zip" => "zip",
        _ => "bin",
    };
    format!("attachment-{position}.{extension}")
}

/// Sanitizes HTML created by the local rich-text composer before it is stored
/// or handed to the SMTP adapter.
///
/// The composer does not currently support embedded remote images, so the
/// strict default policy also prevents accidentally adding tracking content
/// through crafted bridge input.
#[must_use]
pub fn sanitize_composed_html(html: &str) -> String {
    sanitize_html(
        html,
        RenderPolicy::default(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(HashMap::new()),
    )
}

fn sanitize_html(
    html: &str,
    policy: RenderPolicy,
    blocked: Arc<AtomicUsize>,
    inline_images: Arc<HashMap<String, String>>,
) -> String {
    let safe_styles: HashSet<&str> = [
        "background-color",
        "border",
        "border-bottom",
        "border-collapse",
        "border-color",
        "border-left",
        "border-right",
        "border-spacing",
        "border-style",
        "border-top",
        "border-width",
        "color",
        "display",
        "font-family",
        "font-size",
        "font-style",
        "font-weight",
        "height",
        "letter-spacing",
        "line-height",
        "list-style",
        "list-style-type",
        "margin",
        "margin-bottom",
        "margin-left",
        "margin-right",
        "margin-top",
        "max-height",
        "max-width",
        "min-height",
        "min-width",
        "overflow-wrap",
        "padding",
        "padding-bottom",
        "padding-left",
        "padding-right",
        "padding-top",
        "table-layout",
        "text-align",
        "text-decoration",
        "vertical-align",
        "white-space",
        "width",
        "word-break",
    ]
    .into_iter()
    .collect();

    let mut builder = Builder::default();
    builder
        .url_relative(UrlRelative::Deny)
        .add_url_schemes(["cid", "data"])
        .add_generic_attributes([
            "align",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "dir",
            "height",
            "role",
            "style",
            "valign",
            "width",
        ])
        .filter_style_properties(safe_styles)
        .link_rel(Some("noopener noreferrer"))
        .attribute_filter(move |element, attribute, value| {
            let normalized_value = value.to_ascii_lowercase();
            if normalized_value.starts_with("data:") {
                // Sender-provided data URLs are never trusted. The only data
                // URLs returned below are generated from bounded, validated
                // MIME image parts belonging to this message.
                None
            } else if element == "img" && attribute == "src" && normalized_value.starts_with("cid:")
            {
                cid_key_from_url(value)
                    .and_then(|content_id| inline_images.get(&content_id))
                    .cloned()
                    .map(Cow::Owned)
            } else if element == "img"
                && attribute == "src"
                && (normalized_value.starts_with("https://")
                    || normalized_value.starts_with("http://"))
                && !policy.allow_remote_images
            {
                blocked.fetch_add(1, Ordering::Relaxed);
                None
            } else {
                Some(Cow::Borrowed(value))
            }
        });

    builder.clean(html).to_string()
}

fn extract_inline_image_data_uris(message: &mail_parser::Message<'_>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut total_bytes = 0_usize;

    for part in message.attachments() {
        if result.len() >= MAX_INLINE_IMAGES || !is_inline_mime_resource(part) {
            continue;
        }
        let Some(content_id) = part.content_id().and_then(normalize_content_id) else {
            continue;
        };
        let content_type = normalized_attachment_content_type(part);
        let body = part.contents();
        if body.len() > MAX_INLINE_IMAGE_BYTES
            || body.len() > MAX_INLINE_IMAGE_TOTAL_BYTES.saturating_sub(total_bytes)
            || !has_safe_raster_dimensions(&content_type, body)
        {
            continue;
        }
        total_bytes += body.len();
        result.insert(
            content_id,
            format!(
                "data:{content_type};base64,{}",
                BASE64_STANDARD.encode(body)
            ),
        );
    }

    result
}

fn is_inline_mime_resource(part: &mail_parser::MessagePart<'_>) -> bool {
    let disposition = part
        .content_disposition()
        .map(|value| value.c_type.as_ref());
    if disposition.is_some_and(|value| value.eq_ignore_ascii_case("attachment")) {
        return false;
    }
    matches!(part.body, PartType::InlineBinary(_))
        || disposition.is_some_and(|value| value.eq_ignore_ascii_case("inline"))
        || part.content_id().is_some()
}

fn cid_key_from_url(value: &str) -> Option<String> {
    let prefix = value.get(..4)?;
    if !prefix.eq_ignore_ascii_case("cid:") {
        return None;
    }
    let decoded = percent_decode_str(&value[4..]).decode_utf8().ok()?;
    normalize_content_id(&decoded)
}

fn normalize_content_id(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('<').trim_end_matches('>');
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        None
    } else {
        Some(value.to_ascii_lowercase())
    }
}

fn has_safe_raster_dimensions(content_type: &str, body: &[u8]) -> bool {
    let dimensions = match content_type {
        "image/png" => png_dimensions(body),
        "image/gif" => gif_dimensions(body),
        "image/jpeg" => jpeg_dimensions(body),
        _ => None,
    };
    dimensions.is_some_and(|(width, height)| {
        width > 0
            && height > 0
            && width <= MAX_INLINE_IMAGE_DIMENSION
            && height <= MAX_INLINE_IMAGE_DIMENSION
            && u64::from(width) * u64::from(height) <= MAX_INLINE_IMAGE_PIXELS
    })
}

fn png_dimensions(body: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if body.len() < 24 || &body[..8] != PNG_SIGNATURE || &body[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(body[16..20].try_into().ok()?),
        u32::from_be_bytes(body[20..24].try_into().ok()?),
    ))
}

fn gif_dimensions(body: &[u8]) -> Option<(u32, u32)> {
    if body.len() < 10 || (!body.starts_with(b"GIF87a") && !body.starts_with(b"GIF89a")) {
        return None;
    }
    Some((
        u32::from(u16::from_le_bytes(body[6..8].try_into().ok()?)),
        u32::from(u16::from_le_bytes(body[8..10].try_into().ok()?)),
    ))
}

fn jpeg_dimensions(body: &[u8]) -> Option<(u32, u32)> {
    if !body.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut position = 2_usize;
    while position + 4 <= body.len() {
        while position < body.len() && body[position] == 0xff {
            position += 1;
        }
        let marker = *body.get(position)?;
        position += 1;
        if matches!(marker, 0x01 | 0xd8 | 0xd9) || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let segment_length = usize::from(u16::from_be_bytes(
            body.get(position..position + 2)?.try_into().ok()?,
        ));
        if segment_length < 2 || position + segment_length > body.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if segment_length < 7 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes(
                body.get(position + 3..position + 5)?.try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                body.get(position + 5..position + 7)?.try_into().ok()?,
            ));
            return Some((width, height));
        }
        position += segment_length;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{
        AttachmentDecodeError, MAX_CACHED_ATTACHMENTS, MessageRenderer, RenderPolicy,
        decode_attachment_part, has_safe_raster_dimensions, sanitize_composed_html,
    };

    const MULTIPART: &[u8] = br#"From: Anna <anna@example.org>
To: Tim <tim@example.org>
Subject: Standards test
MIME-Version: 1.0
Content-Type: multipart/alternative; boundary=maicenta

--maicenta
Content-Type: text/plain; charset=utf-8

Hello plain
--maicenta
Content-Type: text/html; charset=utf-8

<html><body><table width="600" style="width:600px;position:fixed;background-image:url(https://tracker.example/bg)"><tr><td style="color:#123456;font-family:Arial">Hello <strong>HTML</strong></td></tr></table><script>alert(1)</script><img src="https://tracker.example/pixel" onerror="steal()" alt="pixel"><img src="HTTPS://tracker.example/uppercase" alt="uppercase"><img src="cid:logo@example.org" alt="logo"><a href="javascript:steal()">bad</a><a href="https://example.org">good</a></body></html>
--maicenta--
"#;

    const WITH_ATTACHMENTS: &[u8] = br#"From: Anna <anna@example.org>
To: Tim <tim@example.org>
Subject: Attachment test
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=outer

--outer
Content-Type: text/plain; charset=utf-8

See attachment
--outer
Content-Type: application/pdf
Content-Disposition: attachment; filename="../../Quarterly:Report.pdf"
Content-Transfer-Encoding: base64

UERGIGJvZHk=
--outer
Content-Type: image/png
Content-Disposition: inline
Content-ID: <logo@example.org>
Content-Transfer-Encoding: base64

iVBORw==
--outer--
"#;

    const WITH_INLINE_IMAGE: &[u8] = br#"From: Anna <anna@example.org>
To: Tim <tim@example.org>
Subject: Inline image test
MIME-Version: 1.0
Content-Type: multipart/related; boundary=related

--related
Content-Type: text/html; charset=utf-8

<html><body><p>Logo:</p><img src="cid:logo%40example.org" alt="Logo"></body></html>
--related
Content-Type: image/png
Content-Disposition: inline
Content-ID: <logo@example.org>
Content-Transfer-Encoding: base64

iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=
--related--
"#;

    #[test]
    fn selects_mime_alternatives_and_preserves_email_layout() {
        let rendered = MessageRenderer
            .render(MULTIPART, RenderPolicy::default())
            .expect("valid message");
        let html = rendered.sanitized_html.expect("HTML body");

        assert_eq!(rendered.plain_text.as_deref(), Some("Hello plain"));
        assert!(html.contains("<table width=\"600\""));
        assert!(html.contains("font-family:Arial"));
        assert!(html.contains("color:#123456"));
        assert!(!html.contains("position"));
        assert!(!html.contains("background-image"));
    }

    #[test]
    fn strips_active_content_and_blocks_remote_images_by_default() {
        let rendered = MessageRenderer
            .render(MULTIPART, RenderPolicy::default())
            .expect("valid message");
        let html = rendered.sanitized_html.expect("HTML body");

        assert!(!html.contains("<script"));
        assert!(!html.contains("onerror"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("tracker.example/pixel"));
        assert!(!html.contains("cid:logo@example.org"));
        assert!(html.contains("https://example.org"));
        assert!(html.contains("rel=\"noopener noreferrer\""));
        assert_eq!(rendered.blocked_remote_images, 2);
    }

    #[test]
    fn permits_remote_images_only_when_requested() {
        let rendered = MessageRenderer
            .render(
                MULTIPART,
                RenderPolicy {
                    allow_remote_images: true,
                },
            )
            .expect("valid message");

        assert!(
            rendered
                .sanitized_html
                .expect("HTML body")
                .contains("tracker.example/pixel")
        );
        assert_eq!(rendered.blocked_remote_images, 0);
    }

    #[test]
    fn decodes_transfer_encoding_and_charset() {
        let raw = b"From: a@example.org\r\nContent-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nGr=FC=DFe";
        let rendered = MessageRenderer
            .render(raw, RenderPolicy::default())
            .expect("valid message");

        assert_eq!(rendered.plain_text.as_deref(), Some("Grüße"));
    }

    #[test]
    fn extracts_portable_message_metadata() {
        let rendered = MessageRenderer
            .render(MULTIPART, RenderPolicy::default())
            .expect("valid message");

        assert_eq!(rendered.subject.as_deref(), Some("Standards test"));
        assert_eq!(rendered.from_address.as_deref(), Some("anna@example.org"));
        assert_eq!(rendered.from_display_name.as_deref(), Some("Anna"));
        assert_eq!(rendered.to_recipients, "Tim <tim@example.org>");
        assert!(rendered.cc_recipients.is_empty());
        assert!(rendered.bcc_recipients.is_empty());
    }

    #[test]
    fn decodes_downloadable_attachments_and_excludes_inline_resources() {
        let rendered = MessageRenderer
            .render(WITH_ATTACHMENTS, RenderPolicy::default())
            .expect("valid message");

        assert_eq!(rendered.attachment_count, 1);
        assert!(rendered.attachments_complete);
        assert_eq!(rendered.attachments.len(), 1);
        assert_eq!(rendered.attachments[0].file_name, "Quarterly_Report.pdf");
        assert_eq!(rendered.attachments[0].content_type, "application/pdf");
        assert_eq!(rendered.attachments[0].body, b"PDF body");
    }

    #[test]
    fn resolves_bounded_inline_raster_images_without_network_access() {
        let rendered = MessageRenderer
            .render(WITH_INLINE_IMAGE, RenderPolicy::default())
            .expect("valid message");
        let html = rendered.sanitized_html.expect("HTML body");

        assert_eq!(rendered.attachment_count, 0);
        assert!(rendered.attachments.is_empty());
        assert!(html.contains("src=\"data:image/png;base64,iVBORw0KGgo"));
        assert!(!html.contains("cid:"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
    }

    #[test]
    fn rejects_inline_images_with_excessive_declared_dimensions() {
        let mut png_header = Vec::from(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".as_slice());
        png_header.extend_from_slice(&5000_u32.to_be_bytes());
        png_header.extend_from_slice(&1_u32.to_be_bytes());

        assert!(!has_safe_raster_dimensions("image/png", &png_header));
        assert!(!has_safe_raster_dimensions("image/svg+xml", b"<svg/>"));
    }

    #[test]
    fn does_not_return_a_partial_attachment_set_above_the_count_limit() {
        let mut raw = String::from(
            "From: anna@example.org\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=limit\r\n\r\n",
        );
        raw.push_str("--limit\r\nContent-Type: text/plain\r\n\r\nBody\r\n");
        for position in 0..=MAX_CACHED_ATTACHMENTS {
            write!(
                raw,
                "--limit\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"file-{position}.bin\"\r\n\r\nx\r\n"
            )
            .expect("write MIME fixture");
        }
        raw.push_str("--limit--\r\n");

        let rendered = MessageRenderer
            .render(raw.as_bytes(), RenderPolicy::default())
            .expect("valid message");

        assert_eq!(rendered.attachment_count, MAX_CACHED_ATTACHMENTS + 1);
        assert!(rendered.attachments.is_empty());
        assert!(!rendered.attachments_complete);
    }

    #[test]
    fn sanitizes_locally_composed_html_before_sending() {
        let html = sanitize_composed_html(
            r#"<p style="color:#123456;text-align:center;position:fixed">Hello <strong>world</strong><script>alert(1)</script></p><a href="javascript:alert(1)">unsafe</a><img src="data:image/png;base64,untrusted" alt="untrusted">"#,
        );

        assert!(html.contains("color:#123456"));
        assert!(html.contains("text-align:center"));
        assert!(html.contains("<strong>world</strong>"));
        assert!(!html.contains("position"));
        assert!(!html.contains("script"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("data:image"));
    }

    #[test]
    fn decodes_separately_fetched_mime_sections() {
        assert_eq!(
            decode_attachment_part(b"aW5jb21pbmcg\r\nYnl0ZXM=", "BASE64", 100),
            Ok(b"incoming bytes".to_vec())
        );
        assert_eq!(
            decode_attachment_part(b"Gr=FC=DFe", "quoted-printable", 100),
            Ok(vec![b'G', b'r', 0xfc, 0xdf, b'e'])
        );
    }

    #[test]
    fn bounds_and_rejects_unknown_attachment_encodings() {
        assert_eq!(
            decode_attachment_part(b"abcd", "binary", 3),
            Err(AttachmentDecodeError::SizeLimitExceeded)
        );
        assert_eq!(
            decode_attachment_part(b"abcd", "x-custom", 10),
            Err(AttachmentDecodeError::UnsupportedEncoding)
        );
    }
}
