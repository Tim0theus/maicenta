import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/mail/oauth_client_ids.dart';
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

  test('Graph provider requests Graph mail scopes and the Graph connector', () {
    expect(
      MailOAuthProvider.microsoftGraph.tokenEndpoint,
      MailOAuthProvider.microsoft365.tokenEndpoint,
    );
    expect(
      MailOAuthProvider.microsoftGraph.scopes,
      containsAll([
        'offline_access',
        'https://graph.microsoft.com/Mail.ReadWrite',
        'https://graph.microsoft.com/Mail.Send',
      ]),
    );
    expect(
      MailOAuthProvider.microsoftGraph.scopes,
      isNot(contains('https://outlook.office.com/IMAP.AccessAsUser.All')),
    );
    expect(MailOAuthProvider.microsoftGraph.storageName, 'microsoft_graph');
    expect(MailOAuthProvider.microsoftGraph.mailProvider, 'microsoft_graph');
    expect(MailOAuthProvider.microsoft365.mailProvider, 'imap');
    expect(MailOAuthProvider.google.mailProvider, 'imap');
    expect(
      MailOAuthProviderConfiguration.fromStorageName('microsoft_graph'),
      MailOAuthProvider.microsoftGraph,
    );
    expect(MailOAuthProviderConfiguration.fromStorageName('unknown'), isNull);
  });

  test('falls back to the built-in project client ID without a define', () {
    // Tests run without --dart-define, so the built-in registration applies.
    expect(
      MailOAuthProvider.microsoftGraph.configuredClientId,
      builtInMicrosoftOAuthClientId,
    );
    expect(
      MailOAuthProvider.microsoft365.configuredClientId,
      builtInMicrosoftOAuthClientId,
    );
    expect(
      MailOAuthProvider.google.configuredClientId,
      builtInGoogleOAuthClientId,
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
      // Entra ID accepts custom schemes only as `scheme://host`.
      expect(redirect.host, 'oauth2redirect');
      expect(redirect.toString(), 'com.maicenta.app://oauth2redirect');
    }
  });
}
