import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/mail/oauth_service.dart';

void main() {
  test('uses provider-approved OAuth endpoints and mail scopes', () {
    expect(
      MailOAuthProvider.microsoft365.tokenEndpoint,
      'https://login.microsoftonline.com/common/oauth2/v2.0/token',
    );
    expect(
      MailOAuthProvider.microsoft365.scopes,
      contains('https://outlook.office.com/IMAP.AccessAsUser.All'),
    );
    expect(
      MailOAuthProvider.microsoft365.scopes,
      contains('https://outlook.office.com/SMTP.Send'),
    );
    expect(
      MailOAuthProvider.google.scopes,
      contains('https://mail.google.com/'),
    );
  });

  test('uses loopback only on Windows and Linux desktop', () {
    final redirect = Uri.parse(MailOAuthService.redirectUri);
    if (Platform.isWindows || Platform.isLinux) {
      expect(redirect.scheme, 'http');
      expect(redirect.host, 'localhost');
      expect(redirect.port, 43821);
    } else {
      expect(redirect.scheme, 'com.maicenta.app');
      expect(redirect.path, '/oauth2redirect');
    }
  });
}
