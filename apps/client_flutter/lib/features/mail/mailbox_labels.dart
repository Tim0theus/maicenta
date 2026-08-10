import 'package:flutter/widgets.dart';

import '../../l10n/app_localizations.dart';
import 'mail_data.dart';

String mailboxDisplayName(BuildContext context, MailFolder folder) {
  final localizations = AppLocalizations.of(context);
  return switch (folder.role) {
    'inbox' => localizations.mailboxInbox,
    'drafts' => localizations.mailboxDrafts,
    'sent' => localizations.mailboxSent,
    'archive' => localizations.mailboxArchive,
    'trash' => localizations.mailboxTrash,
    'junk' => localizations.mailboxJunk,
    _ => _customMailboxDisplayName(folder.displayName),
  };
}

String _customMailboxDisplayName(String serverName) {
  for (final prefix in const ['INBOX.', 'INBOX/', 'INBOX\\']) {
    if (serverName.toUpperCase().startsWith(prefix)) {
      return serverName.substring(prefix.length);
    }
  }
  return serverName;
}
