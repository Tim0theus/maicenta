import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:http/http.dart' as http;
import 'package:xml/xml.dart';

typedef MailSettingsDiscovery =
    Future<List<DiscoveredMailSettings>> Function(String emailAddress);

class DiscoveredMailSettings {
  const DiscoveredMailSettings({
    required this.imapHost,
    required this.imapPort,
    required this.imapSecurity,
    required this.imapUsername,
    required this.smtpHost,
    required this.smtpPort,
    required this.smtpSecurity,
    required this.smtpUsername,
    required this.source,
  });

  final String imapHost;
  final int imapPort;
  final String imapSecurity;
  final String imapUsername;
  final String smtpHost;
  final int smtpPort;
  final String smtpSecurity;
  final String smtpUsername;
  final String source;

  String get identity =>
      '$imapHost:$imapPort:$imapSecurity:$imapUsername|'
      '$smtpHost:$smtpPort:$smtpSecurity:$smtpUsername';
}

Future<List<DiscoveredMailSettings>> discoverMailAccountSettings(
  String emailAddress,
) async {
  final client = http.Client();
  try {
    return await MailAccountAutoDiscovery(client).discover(emailAddress);
  } finally {
    client.close();
  }
}

class MailAccountAutoDiscovery {
  MailAccountAutoDiscovery(
    this._client, {
    this.timeout = const Duration(seconds: 8),
  });

  static const _maximumDocumentBytes = 1024 * 1024;

  final http.Client _client;
  final Duration timeout;

  Future<List<DiscoveredMailSettings>> discover(String emailAddress) async {
    final email = emailAddress.trim();
    final domain = _domainFromEmail(email);

    final dnsFuture = _discoverDns(email, domain);
    final configResults = await Future.wait([
      _discoverFromConfig(
        Uri.https(domain, '/.well-known/autoconfig/mail/config-v1.1.xml', {
          'emailaddress': email,
        }),
        email,
        'Domain-Autokonfiguration',
      ),
      _discoverFromConfig(
        Uri.https('autoconfig.$domain', '/mail/config-v1.1.xml', {
          'emailaddress': email,
        }),
        email,
        'Domain-Autokonfiguration',
      ),
      _discoverFromConfig(
        Uri.https('autoconfig.thunderbird.net', '/v1.1/$domain'),
        email,
        'Providerdatenbank',
      ),
    ]);
    final dns = await dnsFuture;

    final discovered = <DiscoveredMailSettings>[
      ...configResults[0],
      ...configResults[1],
      ...dns.settings,
      ...configResults[2],
      ..._knownProviderSettings(email, domain),
      ..._fallbackSettings(email, domain, dns.mxHosts),
    ];
    final unique = <String, DiscoveredMailSettings>{};
    for (final settings in discovered) {
      unique.putIfAbsent(settings.identity, () => settings);
    }
    return unique.values.toList(growable: false);
  }

  Future<List<DiscoveredMailSettings>> _discoverFromConfig(
    Uri uri,
    String email,
    String source,
  ) async {
    final document = await _getText(uri);
    if (document == null) return const [];
    try {
      return parseAutoconfig(document, email: email, source: source);
    } on XmlParserException {
      return const [];
    } on FormatException {
      return const [];
    }
  }

  Future<_DnsDiscovery> _discoverDns(String email, String domain) async {
    final results = await Future.wait([
      _lookupDns('_imaps._tcp.$domain', 'SRV'),
      _lookupDns('_imap._tcp.$domain', 'SRV'),
      _lookupDns('_submissions._tcp.$domain', 'SRV'),
      _lookupDns('_submission._tcp.$domain', 'SRV'),
      _lookupDns(domain, 'MX'),
    ]);
    final imap = <_SrvRecord>[
      ..._parseSrvRecords(results[0], security: 'tls'),
      ..._parseSrvRecords(results[1], security: 'starttls'),
    ];
    final smtp = <_SrvRecord>[
      ..._parseSrvRecords(results[2], security: 'tls'),
      ..._parseSrvRecords(results[3], security: 'starttls'),
    ];
    final settings = <DiscoveredMailSettings>[];
    for (final incoming in imap.take(3)) {
      for (final outgoing in smtp.take(3)) {
        settings.add(
          DiscoveredMailSettings(
            imapHost: incoming.host,
            imapPort: incoming.port,
            imapSecurity: incoming.security,
            imapUsername: email,
            smtpHost: outgoing.host,
            smtpPort: outgoing.port,
            smtpSecurity: outgoing.security,
            smtpUsername: email,
            source: 'DNS-SRV',
          ),
        );
      }
    }
    return _DnsDiscovery(
      settings: settings,
      mxHosts: _parseMxHosts(results[4]),
    );
  }

  Future<Map<String, dynamic>?> _lookupDns(String name, String type) async {
    final response = await _getText(
      Uri.https('cloudflare-dns.com', '/dns-query', {
        'name': name,
        'type': type,
      }),
      accept: 'application/dns-json',
    );
    if (response == null) return null;
    try {
      final decoded = jsonDecode(response);
      return decoded is Map<String, dynamic> ? decoded : null;
    } on FormatException {
      return null;
    }
  }

  Future<String?> _getText(
    Uri initialUri, {
    String accept = 'application/xml,text/xml;q=0.9,*/*;q=0.1',
  }) async {
    try {
      return await _getTextWithRedirects(
        initialUri,
        accept: accept,
      ).timeout(timeout);
    } on Object {
      return null;
    }
  }

  Future<String?> _getTextWithRedirects(
    Uri initialUri, {
    required String accept,
  }) async {
    var uri = initialUri;
    for (var redirect = 0; redirect < 4; redirect += 1) {
      if (uri.scheme != 'https') return null;
      final request = http.Request('GET', uri)
        ..followRedirects = false
        ..headers['Accept'] = accept
        ..headers['User-Agent'] = 'Maicenta/0.1';
      final response = await _client.send(request);
      if (_isRedirect(response.statusCode)) {
        final location = response.headers['location'];
        await _cancelResponse(response);
        if (location == null) return null;
        final redirected = uri.resolve(location);
        if (redirected.scheme != 'https') return null;
        uri = redirected;
        continue;
      }
      if (response.statusCode != 200) {
        await _cancelResponse(response);
        return null;
      }
      final bytes = BytesBuilder(copy: false);
      await for (final chunk in response.stream) {
        bytes.add(chunk);
        if (bytes.length > _maximumDocumentBytes) return null;
      }
      return utf8.decode(bytes.takeBytes(), allowMalformed: true);
    }
    return null;
  }

  static Future<void> _cancelResponse(http.StreamedResponse response) async {
    final subscription = response.stream.listen((_) {});
    await subscription.cancel();
  }

  static bool _isRedirect(int statusCode) =>
      statusCode == 301 ||
      statusCode == 302 ||
      statusCode == 303 ||
      statusCode == 307 ||
      statusCode == 308;

  static String _domainFromEmail(String email) {
    final separator = email.lastIndexOf('@');
    if (separator < 1 || separator != email.indexOf('@')) {
      throw const FormatException('Die E-Mail-Adresse ist ungültig.');
    }
    final domain = email.substring(separator + 1).toLowerCase();
    if (!_isSafeHost(domain)) {
      throw const FormatException('Die E-Mail-Domain ist ungültig.');
    }
    return domain;
  }

  static List<_SrvRecord> _parseSrvRecords(
    Map<String, dynamic>? response, {
    required String security,
  }) {
    final answers = response?['Answer'];
    if (answers is! List) return const [];
    final records = <_SrvRecord>[];
    for (final answer in answers) {
      if (answer is! Map || answer['type'] != 33 || answer['data'] is! String) {
        continue;
      }
      final parts = (answer['data'] as String)
          .replaceAll('"', '')
          .trim()
          .split(RegExp(r'\s+'));
      if (parts.length != 4) continue;
      final priority = int.tryParse(parts[0]);
      final weight = int.tryParse(parts[1]);
      final port = int.tryParse(parts[2]);
      final host = _normalizeHost(parts[3]);
      if (priority == null ||
          weight == null ||
          port == null ||
          port < 1 ||
          port > 65535 ||
          !_isSafeHost(host)) {
        continue;
      }
      records.add(
        _SrvRecord(
          priority: priority,
          weight: weight,
          host: host,
          port: port,
          security: security,
        ),
      );
    }
    records.sort((left, right) {
      final priority = left.priority.compareTo(right.priority);
      return priority != 0 ? priority : right.weight.compareTo(left.weight);
    });
    return records;
  }

  static List<String> _parseMxHosts(Map<String, dynamic>? response) {
    final answers = response?['Answer'];
    if (answers is! List) return const [];
    final hosts = <String>[];
    for (final answer in answers) {
      if (answer is! Map || answer['type'] != 15 || answer['data'] is! String) {
        continue;
      }
      final parts = (answer['data'] as String).trim().split(RegExp(r'\s+'));
      if (parts.length < 2) continue;
      final host = _normalizeHost(parts.last);
      if (_isSafeHost(host) && !hosts.contains(host)) hosts.add(host);
    }
    return hosts;
  }

  static List<DiscoveredMailSettings> _knownProviderSettings(
    String email,
    String domain,
  ) {
    const providers = <String, _ProviderPreset>{
      'gmail.com': _ProviderPreset('imap.gmail.com', 'smtp.gmail.com'),
      'googlemail.com': _ProviderPreset('imap.gmail.com', 'smtp.gmail.com'),
      'outlook.com': _ProviderPreset(
        'outlook.office365.com',
        'smtp-mail.outlook.com',
      ),
      'hotmail.com': _ProviderPreset(
        'outlook.office365.com',
        'smtp-mail.outlook.com',
      ),
      'live.com': _ProviderPreset(
        'outlook.office365.com',
        'smtp-mail.outlook.com',
      ),
      'icloud.com': _ProviderPreset('imap.mail.me.com', 'smtp.mail.me.com'),
      'me.com': _ProviderPreset('imap.mail.me.com', 'smtp.mail.me.com'),
      'mac.com': _ProviderPreset('imap.mail.me.com', 'smtp.mail.me.com'),
      'yahoo.com': _ProviderPreset(
        'imap.mail.yahoo.com',
        'smtp.mail.yahoo.com',
      ),
      'yahoo.de': _ProviderPreset('imap.mail.yahoo.com', 'smtp.mail.yahoo.com'),
      'gmx.de': _ProviderPreset('imap.gmx.net', 'mail.gmx.net'),
      'gmx.net': _ProviderPreset('imap.gmx.net', 'mail.gmx.net'),
      'web.de': _ProviderPreset('imap.web.de', 'smtp.web.de'),
    };
    final provider = providers[domain];
    if (provider == null) return const [];
    return [
      DiscoveredMailSettings(
        imapHost: provider.imapHost,
        imapPort: 993,
        imapSecurity: 'tls',
        imapUsername: email,
        smtpHost: provider.smtpHost,
        smtpPort: 587,
        smtpSecurity: 'starttls',
        smtpUsername: email,
        source: 'Bekannter Anbieter',
      ),
    ];
  }

  static List<DiscoveredMailSettings> _fallbackSettings(
    String email,
    String domain,
    List<String> mxHosts,
  ) {
    final settings = <DiscoveredMailSettings>[];

    void add({
      required String imapHost,
      required String smtpHost,
      required int smtpPort,
      required String smtpSecurity,
      required String source,
    }) {
      if (!_isSafeHost(imapHost) || !_isSafeHost(smtpHost)) return;
      settings.add(
        DiscoveredMailSettings(
          imapHost: imapHost,
          imapPort: 993,
          imapSecurity: 'tls',
          imapUsername: email,
          smtpHost: smtpHost,
          smtpPort: smtpPort,
          smtpSecurity: smtpSecurity,
          smtpUsername: email,
          source: source,
        ),
      );
    }

    add(
      imapHost: 'imap.$domain',
      smtpHost: 'smtp.$domain',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      source: 'Standard-Hostnamen',
    );
    add(
      imapHost: 'mail.$domain',
      smtpHost: 'mail.$domain',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      source: 'Standard-Hostnamen',
    );
    for (final mxHost in mxHosts.take(3)) {
      add(
        imapHost: mxHost,
        smtpHost: mxHost,
        smtpPort: 587,
        smtpSecurity: 'starttls',
        source: 'DNS-MX-Hinweis',
      );
      final labels = mxHost.split('.');
      if (labels.length > 2) {
        final base = labels.skip(1).join('.');
        add(
          imapHost: 'mail.$base',
          smtpHost: 'mail.$base',
          smtpPort: 587,
          smtpSecurity: 'starttls',
          source: 'DNS-MX-Hinweis',
        );
      }
    }
    return settings;
  }

  static String _normalizeHost(String value) {
    final normalized = value.trim().toLowerCase();
    return normalized.endsWith('.')
        ? normalized.substring(0, normalized.length - 1)
        : normalized;
  }

  static bool _isSafeHost(String value) {
    if (value.isEmpty || value.length > 253 || value == 'localhost') {
      return false;
    }
    final labels = value.split('.');
    if (labels.any(
      (label) =>
          label.isEmpty ||
          label.length > 63 ||
          !RegExp(r'^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$').hasMatch(label),
    )) {
      return false;
    }
    return !RegExp(r'^\d+(?:\.\d+){3}$').hasMatch(value);
  }
}

List<DiscoveredMailSettings> parseAutoconfig(
  String definition, {
  required String email,
  required String source,
}) {
  final domain = MailAccountAutoDiscovery._domainFromEmail(email);
  final localPart = email.substring(0, email.lastIndexOf('@'));
  final document = XmlDocument.parse(definition);
  final settings = <DiscoveredMailSettings>[];
  for (final provider in document.findAllElements('emailProvider')) {
    final incoming = provider
        .findElements('incomingServer')
        .where((server) => server.getAttribute('type')?.toLowerCase() == 'imap')
        .map(
          (server) => _parseXmlServer(
            server,
            email: email,
            localPart: localPart,
            domain: domain,
          ),
        )
        .whereType<_ParsedServer>()
        .toList();
    final outgoing = provider
        .findElements('outgoingServer')
        .where((server) => server.getAttribute('type')?.toLowerCase() == 'smtp')
        .map(
          (server) => _parseXmlServer(
            server,
            email: email,
            localPart: localPart,
            domain: domain,
          ),
        )
        .whereType<_ParsedServer>()
        .toList();
    for (final imap in incoming) {
      for (final smtp in outgoing) {
        settings.add(
          DiscoveredMailSettings(
            imapHost: imap.host,
            imapPort: imap.port,
            imapSecurity: imap.security,
            imapUsername: imap.username,
            smtpHost: smtp.host,
            smtpPort: smtp.port,
            smtpSecurity: smtp.security,
            smtpUsername: smtp.username,
            source: source,
          ),
        );
      }
    }
  }
  return settings;
}

_ParsedServer? _parseXmlServer(
  XmlElement element, {
  required String email,
  required String localPart,
  required String domain,
}) {
  String? text(String name) {
    for (final child in element.childElements) {
      if (child.name.local == name) return child.innerText.trim();
    }
    return null;
  }

  String replaceVariables(String value) => value
      .replaceAll('%EMAILADDRESS%', email)
      .replaceAll('%EMAILLOCALPART%', localPart)
      .replaceAll('%EMAILDOMAIN%', domain);

  final hostValue = text('hostname');
  final port = int.tryParse(text('port') ?? '');
  final socketType = text('socketType')?.toUpperCase();
  final security = switch (socketType) {
    'SSL' || 'SSL/TLS' || 'TLS' => 'tls',
    'STARTTLS' => 'starttls',
    _ => null,
  };
  if (hostValue == null ||
      port == null ||
      port < 1 ||
      port > 65535 ||
      security == null) {
    return null;
  }
  final host = MailAccountAutoDiscovery._normalizeHost(
    replaceVariables(hostValue),
  );
  if (!MailAccountAutoDiscovery._isSafeHost(host)) return null;
  final usernameValue = text('username');
  return _ParsedServer(
    host: host,
    port: port,
    security: security,
    username: usernameValue == null || usernameValue.isEmpty
        ? email
        : replaceVariables(usernameValue),
  );
}

class _DnsDiscovery {
  const _DnsDiscovery({required this.settings, required this.mxHosts});

  final List<DiscoveredMailSettings> settings;
  final List<String> mxHosts;
}

class _SrvRecord {
  const _SrvRecord({
    required this.priority,
    required this.weight,
    required this.host,
    required this.port,
    required this.security,
  });

  final int priority;
  final int weight;
  final String host;
  final int port;
  final String security;
}

class _ProviderPreset {
  const _ProviderPreset(this.imapHost, this.smtpHost);

  final String imapHost;
  final String smtpHost;
}

class _ParsedServer {
  const _ParsedServer({
    required this.host,
    required this.port,
    required this.security,
    required this.username,
  });

  final String host;
  final int port;
  final String security;
  final String username;
}
