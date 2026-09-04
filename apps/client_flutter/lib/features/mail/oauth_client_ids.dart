/// Public OAuth client identifiers shipped with MAICENTA.
///
/// MAICENTA is a native public client: it authenticates with Authorization
/// Code + PKCE and never holds a client secret, so these identifiers are not
/// confidential. They identify the MAICENTA app registration to the identity
/// provider the same way Thunderbird or Outlook identify themselves, and let a
/// user sign in without creating their own app registration.
///
/// A `--dart-define` of the same name overrides a built-in value, which keeps
/// forks and development builds independent from the project registration.
///
/// Microsoft: one multi-tenant registration in Microsoft Entra ID ("Accounts
/// in any organizational directory and personal Microsoft accounts") with the
/// redirect URIs `com.maicenta.app:/oauth2redirect` and
/// `http://localhost:43821/oauth2redirect`, public client flows enabled, and
/// the delegated permissions `Mail.ReadWrite`, `Mail.Send`, `offline_access`,
/// `openid`, `profile`, `email` (Microsoft Graph) plus
/// `IMAP.AccessAsUser.All` and `SMTP.Send` (Office 365 Exchange Online).
/// Both Microsoft providers share this registration.
///
/// An empty value means no project registration is available yet; the
/// account dialog then explains how to pass a client ID for development.
library;

/// Application (client) ID of the MAICENTA registration in Microsoft Entra ID.
const String builtInMicrosoftOAuthClientId = '';

/// OAuth client ID of the MAICENTA project in Google Cloud.
const String builtInGoogleOAuthClientId = '';
