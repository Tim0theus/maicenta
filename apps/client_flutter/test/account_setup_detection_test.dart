import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:maicenta/features/mail/account_autodiscovery.dart';
import 'package:maicenta/features/mail/account_setup_detection.dart';

void main() {
  const ownServers = DiscoveredMailSettings(
    imapHost: 'mail.example.org',
    imapPort: 993,
    imapSecurity: 'tls',
    imapUsername: 'user@example.org',
    smtpHost: 'mail.example.org',
    smtpPort: 587,
    smtpSecurity: 'starttls',
    smtpUsername: 'user@example.org',
    source: 'DNS-SRV',
  );

  MockClient client({
    bool microsoftTenant = false,
    String? mx,
    bool autodiscoverSrv = false,
  }) {
    return MockClient((request) async {
      if (request.url.host == 'login.microsoftonline.com') {
        return microsoftTenant
            ? http.Response(
                jsonEncode({
                  'token_endpoint':
                      'https://login.microsoftonline.com/x/oauth2/v2.0/token',
                }),
                200,
              )
            : http.Response(jsonEncode({'error': 'invalid_tenant'}), 400);
      }
      if (request.url.host == 'cloudflare-dns.com') {
        final name = request.url.queryParameters['name']!;
        if (name.startsWith('_autodiscover._tcp.') && autodiscoverSrv) {
          return _dnsResponse(33, '0 0 443 exchange.example.org.');
        }
        if (!name.startsWith('_') && mx != null) {
          return _dnsResponse(15, '10 $mx.');
        }
        return http.Response(jsonEncode({'Status': 0}), 200);
      }
      return http.Response('', 404);
    });
  }

  test('recommends Microsoft Graph for a Microsoft 365 domain', () async {
    final detection = await MailSetupProbe(
      client(
        microsoftTenant: true,
        mx: 'example-org.mail.protection.outlook.com',
      ),
      discover: (_) async => const [ownServers],
    ).detect('user@example.org');

    expect(detection.recommended.method, MailSetupMethod.microsoftGraph);
    expect(detection.summary, contains('Microsoft 365'));
    expect(
      detection.suggestions.map((suggestion) => suggestion.method),
      containsAllInOrder([
        MailSetupMethod.microsoftGraph,
        MailSetupMethod.microsoftImap,
        MailSetupMethod.imapPassword,
        MailSetupMethod.manual,
      ]),
    );
    // The company's own IMAP server stays reachable as an alternative.
    expect(
      detection.suggestions
          .firstWhere(
            (suggestion) => suggestion.method == MailSetupMethod.imapPassword,
          )
          .settings
          ?.imapHost,
      'mail.example.org',
    );
  });

  test('knows personal Microsoft domains without network signals', () async {
    final detection = await MailSetupProbe(
      client(),
      discover: (_) async => const [],
    ).detect('someone@outlook.com');

    expect(detection.recommended.method, MailSetupMethod.microsoftGraph);
    expect(
      detection.suggestions.map((suggestion) => suggestion.method),
      isNot(contains(MailSetupMethod.imapPassword)),
    );
  });

  test('recommends Google sign-in for Google-hosted mail', () async {
    final detection = await MailSetupProbe(
      client(mx: 'aspmx.l.google.com'),
      discover: (_) async => const [ownServers],
    ).detect('user@example.org');

    expect(detection.recommended.method, MailSetupMethod.google);
    expect(detection.recommended.usesOAuthProvider, isTrue);
  });

  test(
    'recommends discovered IMAP servers with a password otherwise',
    () async {
      final detection = await MailSetupProbe(
        client(mx: 'mx.example.net'),
        discover: (_) async => const [ownServers],
      ).detect('user@example.org');

      expect(detection.recommended.method, MailSetupMethod.imapPassword);
      expect(detection.recommended.settings?.smtpHost, 'mail.example.org');
      expect(detection.suggestions.last.method, MailSetupMethod.manual);
    },
  );

  test('falls back to manual setup and flags on-premises Exchange', () async {
    final detection = await MailSetupProbe(
      client(autodiscoverSrv: true),
      discover: (_) async => const [],
    ).detect('user@example.org');

    expect(detection.recommended.method, MailSetupMethod.manual);
    expect(detection.summary, contains('kein Anbieter'));
    expect(
      detection.suggestions.last.method,
      MailSetupMethod.exchangeOnPremises,
    );
    expect(detection.suggestions.last.method.isSupported, isFalse);
  });

  test('treats a failing autodiscovery as no servers', () async {
    final detection = await MailSetupProbe(
      client(),
      discover: (_) async => throw StateError('offline'),
    ).detect('user@example.org');

    expect(detection.recommended.method, MailSetupMethod.manual);
  });

  test('existing accounts keep their method as the recommendation', () {
    final detection = MailSetupDetection.forExistingAccount(
      emailAddress: 'user@example.org',
      method: MailSetupMethod.microsoftImap,
      storedSettings: ownServers,
    );

    expect(detection.recommended.method, MailSetupMethod.microsoftImap);
    expect(
      detection.suggestions.map((suggestion) => suggestion.method),
      isNot(contains(MailSetupMethod.exchangeOnPremises)),
    );
    expect(
      detection.suggestions
          .firstWhere(
            (suggestion) => suggestion.method == MailSetupMethod.manual,
          )
          .settings,
      ownServers,
    );
  });

  test('rejects malformed addresses before probing', () {
    expect(
      () => MailSetupProbe(client()).detect('invalid'),
      throwsFormatException,
    );
  });
}

extension on MailSetupSuggestion {
  bool get usesOAuthProvider => method.usesOAuth;
}

http.Response _dnsResponse(int type, String data) => http.Response(
  jsonEncode({
    'Status': 0,
    'Answer': [
      {'name': 'example.org.', 'type': type, 'TTL': 300, 'data': data},
    ],
  }),
  200,
);
