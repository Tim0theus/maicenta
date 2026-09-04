import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:flutter_web_auth_2/flutter_web_auth_2.dart';
import 'package:http/http.dart' as http;

import 'oauth_client_ids.dart';

/// OAuth providers MAICENTA can sign in to.
///
/// `microsoft365` and `microsoftGraph` share the same Microsoft identity
/// platform and app registration but request tokens for different resources:
/// the IMAP/SMTP scopes of Exchange Online versus the Microsoft Graph mail
/// API. A token for one resource cannot be used for the other.
enum MailOAuthProvider { microsoft365, microsoftGraph, google }

extension MailOAuthProviderConfiguration on MailOAuthProvider {
  String get storageName => switch (this) {
    MailOAuthProvider.microsoft365 => 'microsoft365',
    MailOAuthProvider.microsoftGraph => 'microsoft_graph',
    MailOAuthProvider.google => 'google',
  };

  static MailOAuthProvider? fromStorageName(String? value) => switch (value) {
    'microsoft365' => MailOAuthProvider.microsoft365,
    'microsoft_graph' => MailOAuthProvider.microsoftGraph,
    'google' => MailOAuthProvider.google,
    _ => null,
  };

  String get displayName => switch (this) {
    MailOAuthProvider.microsoft365 =>
      'Microsoft 365 / Exchange Online (IMAP/SMTP)',
    MailOAuthProvider.microsoftGraph =>
      'Microsoft 365 / Exchange Online (Graph API)',
    MailOAuthProvider.google => 'Google Workspace / Gmail',
  };

  /// Mail connector the Rust core uses for accounts signed in with this
  /// provider.
  String get mailProvider => switch (this) {
    MailOAuthProvider.microsoftGraph => 'microsoft_graph',
    MailOAuthProvider.microsoft365 || MailOAuthProvider.google => 'imap',
  };

  bool get usesMicrosoftIdentity =>
      this == MailOAuthProvider.microsoft365 ||
      this == MailOAuthProvider.microsoftGraph;

  String get authorizationEndpoint => switch (this) {
    MailOAuthProvider.microsoft365 || MailOAuthProvider.microsoftGraph =>
      'https://login.microsoftonline.com/common/oauth2/v2.0/authorize',
    MailOAuthProvider.google => 'https://accounts.google.com/o/oauth2/v2/auth',
  };

  String get tokenEndpoint => switch (this) {
    MailOAuthProvider.microsoft365 || MailOAuthProvider.microsoftGraph =>
      'https://login.microsoftonline.com/common/oauth2/v2.0/token',
    MailOAuthProvider.google => 'https://oauth2.googleapis.com/token',
  };

  List<String> get scopes => switch (this) {
    MailOAuthProvider.microsoft365 => const [
      'openid',
      'profile',
      'email',
      'offline_access',
      'https://outlook.office.com/IMAP.AccessAsUser.All',
      'https://outlook.office.com/SMTP.Send',
    ],
    MailOAuthProvider.microsoftGraph => const [
      'openid',
      'profile',
      'email',
      'offline_access',
      'https://graph.microsoft.com/Mail.ReadWrite',
      'https://graph.microsoft.com/Mail.Send',
    ],
    MailOAuthProvider.google => const [
      'openid',
      'email',
      'https://mail.google.com/',
    ],
  };

  String get clientIdDefineName => switch (this) {
    MailOAuthProvider.microsoft365 ||
    MailOAuthProvider.microsoftGraph => 'MAICENTA_MICROSOFT_OAUTH_CLIENT_ID',
    MailOAuthProvider.google => 'MAICENTA_GOOGLE_OAUTH_CLIENT_ID',
  };

  /// Client ID compiled into this build.
  ///
  /// A `--dart-define` override wins; otherwise the project's public
  /// registration from `oauth_client_ids.dart` is used.
  String get configuredClientId {
    final override = switch (this) {
      MailOAuthProvider.microsoft365 || MailOAuthProvider.microsoftGraph =>
        const String.fromEnvironment('MAICENTA_MICROSOFT_OAUTH_CLIENT_ID'),
      MailOAuthProvider.google => const String.fromEnvironment(
        'MAICENTA_GOOGLE_OAUTH_CLIENT_ID',
      ),
    };
    if (override.trim().isNotEmpty) return override.trim();
    return switch (this) {
      MailOAuthProvider.microsoft365 ||
      MailOAuthProvider.microsoftGraph => builtInMicrosoftOAuthClientId,
      MailOAuthProvider.google => builtInGoogleOAuthClientId,
    };
  }
}

class MailOAuthTokens {
  const MailOAuthTokens({
    required this.provider,
    required this.clientId,
    required this.accessToken,
    required this.refreshToken,
    required this.expiresAt,
    required this.tokenEndpoint,
    required this.scopes,
  });

  final MailOAuthProvider provider;
  final String clientId;
  final String accessToken;
  final String refreshToken;
  final DateTime expiresAt;
  final String tokenEndpoint;
  final String scopes;
}

class MailOAuthService {
  MailOAuthService({http.Client? client}) : _client = client ?? http.Client();

  static const _configuredRedirectUri = String.fromEnvironment(
    'MAICENTA_OAUTH_REDIRECT_URI',
  );

  static String get redirectUri {
    if (_configuredRedirectUri.isNotEmpty) return _configuredRedirectUri;
    if (Platform.isWindows || Platform.isLinux) {
      return 'http://localhost:43821/oauth2redirect';
    }
    return 'com.maicenta.app:/oauth2redirect';
  }

  final http.Client _client;

  Future<MailOAuthTokens> authorize({
    required MailOAuthProvider provider,
    required String loginHint,
  }) async {
    final clientId = provider.configuredClientId.trim();
    if (clientId.isEmpty) {
      final defineName = provider.clientIdDefineName;
      throw StateError(
        'Dieser Build enthält noch keine OAuth-App-ID für '
        '${provider.displayName}. Offizielle Builds bringen die '
        'MAICENTA-Registrierung mit; für einen eigenen Build starte mit '
        '--dart-define=$defineName=<Client-ID>.',
      );
    }

    final callback = Uri.parse(redirectUri);
    final loopbackCallback =
        callback.scheme == 'http' &&
        (callback.host == 'localhost' || callback.host == '127.0.0.1') &&
        callback.hasPort;
    if (callback.scheme.isEmpty ||
        (callback.scheme == 'http' && !loopbackCallback)) {
      throw StateError('Die OAuth-Redirect-URI ist für diese App ungültig.');
    }
    final state = _randomUrlSafe(32);
    final verifier = _randomUrlSafe(64);
    final challenge = base64Url
        .encode(sha256.convert(ascii.encode(verifier)).bytes)
        .replaceAll('=', '');
    final scope = provider.scopes.join(' ');
    final parameters = <String, String>{
      'client_id': clientId,
      'redirect_uri': redirectUri,
      'response_type': 'code',
      'scope': scope,
      'state': state,
      'code_challenge': challenge,
      'code_challenge_method': 'S256',
      'login_hint': loginHint.trim(),
      'prompt': provider == MailOAuthProvider.google
          ? 'consent select_account'
          : 'select_account',
    };
    if (provider == MailOAuthProvider.google) {
      parameters['access_type'] = 'offline';
      parameters['include_granted_scopes'] = 'true';
    }
    final authorizationUri = Uri.parse(
      provider.authorizationEndpoint,
    ).replace(queryParameters: parameters);
    final result = await FlutterWebAuth2.authenticate(
      url: authorizationUri.toString(),
      callbackUrlScheme: loopbackCallback ? redirectUri : callback.scheme,
      options: FlutterWebAuth2Options(useWebview: !loopbackCallback),
    );
    final returned = Uri.parse(result);
    if (returned.scheme != callback.scheme ||
        returned.path != callback.path ||
        (loopbackCallback &&
            (returned.host != callback.host ||
                returned.port != callback.port))) {
      throw StateError(
        'Die OAuth-Antwort verwendete einen ungültigen Rücksprung.',
      );
    }
    if (returned.queryParameters['state'] != state) {
      throw StateError('Die OAuth-Antwort hatte einen ungültigen Statuswert.');
    }
    final providerError = returned.queryParameters['error'];
    if (providerError != null) {
      throw StateError(
        returned.queryParameters['error_description'] ?? providerError,
      );
    }
    final code = returned.queryParameters['code'];
    if (code == null || code.isEmpty) {
      throw StateError(
        'Der OAuth-Anbieter hat keinen Autorisierungscode geliefert.',
      );
    }

    final response = await _client.post(
      Uri.parse(provider.tokenEndpoint),
      headers: const {'Accept': 'application/json'},
      body: {
        'client_id': clientId,
        'code': code,
        'code_verifier': verifier,
        'grant_type': 'authorization_code',
        'redirect_uri': redirectUri,
        'scope': scope,
      },
    );
    final payload = _jsonObject(response.body);
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw StateError(
        payload['error_description']?.toString() ??
            payload['error']?.toString() ??
            'Der OAuth-Anbieter hat den Token-Austausch abgelehnt.',
      );
    }
    final accessToken = payload['access_token']?.toString() ?? '';
    final refreshToken = payload['refresh_token']?.toString() ?? '';
    final expiresIn = int.tryParse(payload['expires_in']?.toString() ?? '');
    if (accessToken.isEmpty || refreshToken.isEmpty || expiresIn == null) {
      throw StateError(
        'Der OAuth-Anbieter hat keinen vollständig erneuerbaren Token-Satz geliefert.',
      );
    }
    return MailOAuthTokens(
      provider: provider,
      clientId: clientId,
      accessToken: accessToken,
      refreshToken: refreshToken,
      expiresAt: DateTime.now().toUtc().add(Duration(seconds: expiresIn)),
      tokenEndpoint: provider.tokenEndpoint,
      scopes: scope,
    );
  }

  static Map<String, dynamic> _jsonObject(String value) {
    try {
      final decoded = jsonDecode(value);
      if (decoded is Map<String, dynamic>) return decoded;
    } on FormatException {
      // The caller presents a provider-neutral error below.
    }
    return const {};
  }

  static String _randomUrlSafe(int byteCount) {
    final random = Random.secure();
    final bytes = List<int>.generate(byteCount, (_) => random.nextInt(256));
    return base64Url.encode(bytes).replaceAll('=', '');
  }
}
