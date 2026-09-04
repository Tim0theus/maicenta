import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:maicenta/features/mail/account_autodiscovery.dart';

void main() {
  test('parses secure Mozilla autoconfig settings and usernames', () {
    final settings = parseAutoconfig(
      _autoconfig,
      email: 'user@example.org',
      source: 'Test',
    );

    expect(settings, hasLength(1));
    expect(settings.single.imapHost, 'imap.example.org');
    expect(settings.single.imapPort, 993);
    expect(settings.single.imapSecurity, 'tls');
    expect(settings.single.imapUsername, 'user');
    expect(settings.single.smtpHost, 'smtp.example.org');
    expect(settings.single.smtpPort, 587);
    expect(settings.single.smtpSecurity, 'starttls');
    expect(settings.single.smtpUsername, 'user@example.org');
  });

  test('prefers a domain autoconfig document', () async {
    final client = MockClient((request) async {
      if (request.url.host == 'autoconfig.example.org') {
        return http.Response(_autoconfig, 200);
      }
      if (request.url.host == 'cloudflare-dns.com') {
        return http.Response(jsonEncode({'Status': 0}), 200);
      }
      return http.Response('', 404);
    });

    final settings = await MailAccountAutoDiscovery(
      client,
    ).discover('user@example.org');

    expect(settings.first.source, 'Domain-Autokonfiguration');
    expect(settings.first.imapHost, 'imap.example.org');
    expect(settings.first.smtpHost, 'smtp.example.org');
  });

  test('uses secure IMAP and submission SRV records', () async {
    final client = MockClient((request) async {
      if (request.url.host != 'cloudflare-dns.com') {
        return http.Response('', 404);
      }
      final name = request.url.queryParameters['name'];
      if (name == '_imaps._tcp.example.org') {
        return _dnsResponse(33, '0 100 993 mail.example.net.');
      }
      if (name == '_submission._tcp.example.org') {
        return _dnsResponse(33, '0 100 587 mail.example.net.');
      }
      if (name == 'example.org') {
        return _dnsResponse(15, '10 mx.example.net.');
      }
      return http.Response(jsonEncode({'Status': 0}), 200);
    });

    final settings = await MailAccountAutoDiscovery(
      client,
    ).discover('user@example.org');

    expect(settings.first.source, 'DNS-SRV');
    expect(settings.first.imapHost, 'mail.example.net');
    expect(settings.first.imapPort, 993);
    expect(settings.first.imapSecurity, 'tls');
    expect(settings.first.smtpHost, 'mail.example.net');
    expect(settings.first.smtpPort, 587);
    expect(settings.first.smtpSecurity, 'starttls');
  });

  test('rejects malformed email domains before network discovery', () async {
    final client = MockClient((_) async => http.Response('', 500));

    expect(
      () => MailAccountAutoDiscovery(client).discover('invalid'),
      throwsFormatException,
    );
  });
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

const _autoconfig = '''
<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="example.org">
    <domain>example.org</domain>
    <displayName>Example Mail</displayName>
    <incomingServer type="imap">
      <hostname>imap.%EMAILDOMAIN%</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILLOCALPART%</username>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.%EMAILDOMAIN%</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <authentication>password-cleartext</authentication>
      <username>%EMAILADDRESS%</username>
    </outgoingServer>
  </emailProvider>
</clientConfig>
''';
