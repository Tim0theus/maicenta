import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

/// Matches the inert scheme emitted by the Rust renderer for remote images.
///
/// The original HTTP(S) URL is percent encoded. It remains locally available
/// for an explicit user action, but cannot trigger a network request by itself.
const blockedRemoteImageScheme = 'maicenta-blocked-image:';

bool hasBlockedRemoteImages(String html) =>
    html.contains(blockedRemoteImageScheme);

/// Detects image elements from older cache entries where the renderer removed
/// the source URL completely. Re-fetching the MIME body can upgrade them to an
/// inert, explicitly loadable reference.
bool hasUnresolvedMessageImages(String html) {
  final sourceAttribute = RegExp(r'\bsrc\s*=', caseSensitive: false);
  return RegExp(
    r'<img\b[^>]*>',
    caseSensitive: false,
  ).allMatches(html).any((match) => !sourceAttribute.hasMatch(match.group(0)!));
}

/// Returns whether the cached body contains text a user can actually read.
///
/// This deliberately ignores image alternative attributes. It lets the UI
/// recognize old image-only messages whose remote sources were removed before
/// the inert URL scheme was introduced and offer an IMAP refresh for them.
bool hasDisplayableMessageText(String html, String plainText) {
  if (plainText.trim().isNotEmpty) return true;
  final visible = html
      .replaceAll(RegExp(r'<!--[\s\S]*?-->'), ' ')
      .replaceAll(RegExp(r'<[^>]*>'), ' ')
      .replaceAll(RegExp(r'&(nbsp|zwnj|zwj);', caseSensitive: false), ' ')
      .replaceAll(RegExp(r'&#(160|8204|8205);'), ' ')
      .replaceAll(RegExp(r'\s+'), '')
      .trim();
  return visible.isNotEmpty;
}

/// Widget factory that treats all network images as blocked until explicitly
/// allowed. Even then, only the HTTP(S) URL encoded by the trusted Rust
/// renderer is accepted; arbitrary or malformed schemes remain disabled.
class SafeMailWidgetFactory extends WidgetFactory {
  SafeMailWidgetFactory({required this.allowRemoteImages});

  final bool Function() allowRemoteImages;

  @override
  ImageProvider? imageProviderFromNetwork(String url) {
    if (!allowRemoteImages()) return null;

    var candidate = url;
    if (candidate.startsWith(blockedRemoteImageScheme)) {
      final encoded = candidate.substring(blockedRemoteImageScheme.length);
      try {
        candidate = Uri.decodeComponent(encoded);
      } on FormatException {
        return null;
      }
    }

    final uri = Uri.tryParse(candidate);
    if (uri == null ||
        !uri.hasAuthority ||
        (uri.scheme != 'https' && uri.scheme != 'http')) {
      return null;
    }
    return super.imageProviderFromNetwork(candidate);
  }
}
