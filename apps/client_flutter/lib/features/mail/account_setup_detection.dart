import 'dart:async';
import 'dart:convert';

import 'package:http/http.dart' as http;

import 'account_autodiscovery.dart';
import 'oauth_service.dart';

/// How a mail account can be connected.
///
/// The account dialog shows these as plain-language choices. Technical terms
/// such as OAuth, Graph, or XOAUTH2 stay behind the "Erweitert" section.
enum MailSetupMethod {
  /// Sign in with a Microsoft account; synchronize through Microsoft Graph.
  microsoftGraph,

  /// Sign in with a Microsoft account; synchronize through IMAP/SMTP.
  microsoftImap,

  /// Sign in with a Google account; synchronize through IMAP/SMTP.
  google,

  /// Automatically discovered IMAP/SMTP servers with a mail password.
  imapPassword,

  /// User-entered IMAP/SMTP servers with a mail password.
  manual,

  /// An on-premises Exchange server was detected. Not supported yet.
  exchangeOnPremises,
}

extension MailSetupMethodPresentation on MailSetupMethod {
  String get title => switch (this) {
    MailSetupMethod.microsoftGraph => 'Microsoft 365 / Exchange Online',
    MailSetupMethod.microsoftImap => 'Microsoft 365 klassisch (IMAP/SMTP)',
    MailSetupMethod.google => 'Google Workspace / Gmail',
    MailSetupMethod.imapPassword => 'E-Mail-Server mit Passwort',
    MailSetupMethod.manual => 'Servereinstellungen selbst eingeben',
    MailSetupMethod.exchangeOnPremises => 'Lokaler Exchange-Server erkannt',
  };

  String get description => switch (this) {
    MailSetupMethod.microsoftGraph =>
      'Anmeldung mit deinem Microsoft-Konto im Browser. Funktioniert auch, '
          'wenn dein Unternehmen den klassischen Mailabruf abgeschaltet hat.',
    MailSetupMethod.microsoftImap =>
      'Anmeldung mit deinem Microsoft-Konto, Abruf über die klassischen '
          'Mailprotokolle. Nur nötig, wenn die empfohlene Variante nicht '
          'freigegeben ist.',
    MailSetupMethod.google =>
      'Anmeldung mit deinem Google-Konto im Browser. Kein App-Passwort nötig.',
    MailSetupMethod.imapPassword =>
      'Die Server wurden automatisch erkannt. Du meldest dich mit dem '
          'Passwort deines E-Mail-Postfachs an.',
    MailSetupMethod.manual =>
      'Für Sonderfälle oder wenn die automatische Erkennung nichts gefunden '
          'hat.',
    MailSetupMethod.exchangeOnPremises =>
      'Diese Version unterstützt lokale Exchange-Server noch nicht. Frag '
          'deine Administration nach einem IMAP-Zugang.',
  };

  /// Whether the method signs in through a browser instead of a password.
  bool get usesOAuth => oauthProvider != null;

  MailOAuthProvider? get oauthProvider => switch (this) {
    MailSetupMethod.microsoftGraph => MailOAuthProvider.microsoftGraph,
    MailSetupMethod.microsoftImap => MailOAuthProvider.microsoft365,
    MailSetupMethod.google => MailOAuthProvider.google,
    MailSetupMethod.imapPassword ||
    MailSetupMethod.manual ||
    MailSetupMethod.exchangeOnPremises => null,
  };

  /// Connector the Rust core uses for accounts created with this method.
  String get mailProvider => switch (this) {
    MailSetupMethod.microsoftGraph => 'microsoft_graph',
    _ => 'imap',
  };

  bool get isSupported => this != MailSetupMethod.exchangeOnPremises;

  /// Technical summary for the "Erweitert" section.
  String get technicalSummary => switch (this) {
    MailSetupMethod.microsoftGraph =>
      'OAuth 2.0 mit PKCE, Microsoft Graph API (Mail.ReadWrite, Mail.Send). '
          'Keine Push-Benachrichtigungen; regelmäßiger Abgleich.',
    MailSetupMethod.microsoftImap =>
      'OAuth 2.0 mit PKCE, IMAP und SMTP mit XOAUTH2 über '
          'outlook.office365.com und smtp.office365.com.',
    MailSetupMethod.google =>
      'OAuth 2.0 mit PKCE, IMAP und SMTP mit XOAUTH2 über imap.gmail.com und '
          'smtp.gmail.com.',
    MailSetupMethod.imapPassword =>
      'IMAP und SMTP mit Passwort. Server aus Domain-Autokonfiguration, '
          'DNS oder Providerdatenbank.',
    MailSetupMethod.manual => 'IMAP und SMTP mit Passwort, Server manuell.',
    MailSetupMethod.exchangeOnPremises =>
      'DNS-Eintrag _autodiscover._tcp gefunden. EWS wird nicht unterstützt.',
  };
}

/// One way the detected account could be connected.
class MailSetupSuggestion {
  const MailSetupSuggestion({
    required this.method,
    this.settingsCandidates = const [],
    this.recommended = false,
  });

  final MailSetupMethod method;

  /// Ordered IMAP/SMTP candidates for password-based methods. The dialog
  /// verifies them in order and keeps the first that accepts the password.
  final List<DiscoveredMailSettings> settingsCandidates;
  final bool recommended;

  DiscoveredMailSettings? get settings =>
      settingsCandidates.isEmpty ? null : settingsCandidates.first;
}

/// Result of probing one e-mail address.
class MailSetupDetection {
  const MailSetupDetection({
    required this.emailAddress,
    required this.suggestions,
    required this.summary,
  });

  final String emailAddress;

  /// Recommended suggestion first, unsupported hints last.
  final List<MailSetupSuggestion> suggestions;

  /// Plain-language explanation of what was detected.
  final String summary;

  MailSetupSuggestion get recommended => suggestions.firstWhere(
    (suggestion) => suggestion.recommended,
    orElse: () => suggestions.first,
  );

  /// Suggestions for an account that already exists: the stored method is
  /// recommended, every other supported method stays reachable.
  static MailSetupDetection forExistingAccount({
    required String emailAddress,
    required MailSetupMethod method,
    required DiscoveredMailSettings storedSettings,
  }) {
    final suggestions = <MailSetupSuggestion>[
      MailSetupSuggestion(
        method: method,
        settingsCandidates: [storedSettings],
        recommended: true,
      ),
      for (final other in MailSetupMethod.values)
        if (other != method && other.isSupported)
          MailSetupSuggestion(
            method: other,
            settingsCandidates: other.usesOAuth ? const [] : [storedSettings],
          ),
    ];
    return MailSetupDetection(
      emailAddress: emailAddress,
      suggestions: suggestions,
      summary: 'Gespeicherte Einrichtung: ${method.title}',
    );
  }
}

typedef MailSetupDetector =
    Future<MailSetupDetection> Function(String emailAddress);

/// Detects how [emailAddress] can be connected using a temporary HTTP client.
Future<MailSetupDetection> detectMailSetup(String emailAddress) async {
  final client = http.Client();
  try {
    return await MailSetupProbe(client).detect(emailAddress);
  } finally {
    client.close();
  }
}

/// Probes public, password-free signals for one e-mail domain.
///
/// - Microsoft 365: the tenant discovery endpoint answers for every domain
///   registered in Entra ID, and Exchange Online MX records end in
///   `mail.protection.outlook.com`. Personal Microsoft domains are known.
/// - Google: Gmail domains or MX records pointing to Google.
/// - IMAP/SMTP: the regular autodiscovery (autoconfig, DNS SRV, ISPDB).
/// - On-premises Exchange: an `_autodiscover._tcp` SRV record without any
///   Microsoft 365 signal.
class MailSetupProbe {
  MailSetupProbe(
    this._client, {
    this.discover,
    this.timeout = const Duration(seconds: 6),
  });

  static const _microsoftPersonalDomains = {
    'outlook.com',
    'outlook.de',
    'hotmail.com',
    'hotmail.de',
    'live.com',
    'live.de',
    'msn.com',
  };
  static const _googleDomains = {'gmail.com', 'googlemail.com'};
  static const _microsoftMxSuffixes = [
    '.mail.protection.outlook.com',
    '.olc.protection.outlook.com',
    '.mail.eo.outlook.com',
  ];
  static const _googleMxSuffixes = ['.google.com', '.googlemail.com'];

  final http.Client _client;

  /// Overrides the IMAP/SMTP autodiscovery, for example in tests.
  final MailSettingsDiscovery? discover;
  final Duration timeout;

  Future<MailSetupDetection> detect(String emailAddress) async {
    final email = emailAddress.trim();
    final domain = mailDomainOf(email);

    final results = await Future.wait<Object?>([
      _probeMicrosoftTenant(domain),
      _lookupDnsData(domain, 'MX', 15),
      _lookupDnsData('_autodiscover._tcp.$domain', 'SRV', 33),
      _discoverSettings(email),
    ]);
    final tenantFound = results[0] as bool;
    final mxHosts = (results[1] as List<String>)
        .map(_mxTarget)
        .whereType<String>()
        .toList(growable: false);
    final autodiscoverSrv = (results[2] as List<String>).isNotEmpty;
    final settings = results[3] as List<DiscoveredMailSettings>;

    final isMicrosoft =
        _microsoftPersonalDomains.contains(domain) ||
        tenantFound ||
        mxHosts.any(
          (host) => _microsoftMxSuffixes.any((suffix) => host.endsWith(suffix)),
        );
    final isGoogle =
        _googleDomains.contains(domain) ||
        mxHosts.any(
          (host) => _googleMxSuffixes.any((suffix) => host.endsWith(suffix)),
        );

    final suggestions = <MailSetupSuggestion>[];
    final String summary;
    if (isMicrosoft) {
      summary = _microsoftPersonalDomains.contains(domain)
          ? 'Microsoft-Konto erkannt.'
          : 'Die Domain $domain ist bei Microsoft 365 registriert.';
      suggestions.add(
        const MailSetupSuggestion(
          method: MailSetupMethod.microsoftGraph,
          recommended: true,
        ),
      );
      suggestions.add(
        const MailSetupSuggestion(method: MailSetupMethod.microsoftImap),
      );
      final ownServers = settings
          .where((candidate) => !_isMicrosoftHost(candidate.imapHost))
          .toList(growable: false);
      if (ownServers.isNotEmpty) {
        suggestions.add(
          MailSetupSuggestion(
            method: MailSetupMethod.imapPassword,
            settingsCandidates: ownServers,
          ),
        );
      }
    } else if (isGoogle) {
      summary = 'Google-Konto erkannt.';
      suggestions.add(
        const MailSetupSuggestion(
          method: MailSetupMethod.google,
          recommended: true,
        ),
      );
      if (settings.isNotEmpty) {
        suggestions.add(
          MailSetupSuggestion(
            method: MailSetupMethod.imapPassword,
            settingsCandidates: settings,
          ),
        );
      }
    } else if (settings.isNotEmpty) {
      summary =
          'Servereinstellungen für $domain gefunden (${settings.first.source}).';
      suggestions.add(
        MailSetupSuggestion(
          method: MailSetupMethod.imapPassword,
          settingsCandidates: settings,
          recommended: true,
        ),
      );
    } else {
      summary = 'Für $domain wurde kein Anbieter automatisch erkannt.';
    }
    suggestions.add(
      MailSetupSuggestion(
        method: MailSetupMethod.manual,
        settingsCandidates: settings,
        recommended: suggestions.isEmpty,
      ),
    );
    if (autodiscoverSrv && !isMicrosoft) {
      suggestions.add(
        const MailSetupSuggestion(method: MailSetupMethod.exchangeOnPremises),
      );
    }
    return MailSetupDetection(
      emailAddress: email,
      suggestions: suggestions,
      summary: summary,
    );
  }

  Future<List<DiscoveredMailSettings>> _discoverSettings(String email) async {
    try {
      final discover = this.discover;
      if (discover != null) return await discover(email);
      return await MailAccountAutoDiscovery(
        _client,
        timeout: timeout,
      ).discover(email);
    } on Object {
      return const [];
    }
  }

  /// Entra ID answers with the tenant's OpenID configuration for every
  /// registered domain and with an error for unknown domains.
  Future<bool> _probeMicrosoftTenant(String domain) async {
    try {
      final response = await _client
          .get(
            Uri.https(
              'login.microsoftonline.com',
              '/$domain/v2.0/.well-known/openid-configuration',
            ),
            headers: const {
              'Accept': 'application/json',
              'User-Agent': 'Maicenta/0.1',
            },
          )
          .timeout(timeout);
      if (response.statusCode != 200) return false;
      final decoded = jsonDecode(response.body);
      return decoded is Map<String, dynamic> &&
          decoded['token_endpoint'] is String;
    } on Object {
      return false;
    }
  }

  Future<List<String>> _lookupDnsData(
    String name,
    String type,
    int recordType,
  ) async {
    try {
      final response = await _client
          .get(
            Uri.https('cloudflare-dns.com', '/dns-query', {
              'name': name,
              'type': type,
            }),
            headers: const {
              'Accept': 'application/dns-json',
              'User-Agent': 'Maicenta/0.1',
            },
          )
          .timeout(timeout);
      if (response.statusCode != 200) return const [];
      final decoded = jsonDecode(response.body);
      final answers = decoded is Map<String, dynamic>
          ? decoded['Answer']
          : null;
      if (answers is! List) return const [];
      return [
        for (final answer in answers)
          if (answer is Map &&
              answer['type'] == recordType &&
              answer['data'] is String)
            answer['data'] as String,
      ];
    } on Object {
      return const [];
    }
  }

  static String? _mxTarget(String data) {
    final parts = data.trim().split(RegExp(r'\s+'));
    if (parts.length < 2) return null;
    final host = parts.last.toLowerCase();
    return host.endsWith('.') ? host.substring(0, host.length - 1) : host;
  }

  static bool _isMicrosoftHost(String host) =>
      host.endsWith('office365.com') ||
      host.endsWith('outlook.com') ||
      host.endsWith('outlook.office.com');
}
