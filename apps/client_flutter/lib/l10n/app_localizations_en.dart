// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get mailModule => 'Mail';

  @override
  String get favorites => 'Favorites';

  @override
  String get localArea => 'Local area';

  @override
  String get localDemoMode => 'Local demo mode';

  @override
  String mailAccountsConnected(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count mail accounts connected',
      one: '1 mail account connected',
    );
    return '$_temp0';
  }

  @override
  String get unreadEmails => 'Unread Mail';

  @override
  String get followUp => 'For Follow Up';

  @override
  String get accountMenu => 'Account menu';

  @override
  String get accountSettings => 'Account Settings';

  @override
  String get newLocalFolder => 'New local folder';

  @override
  String get mailboxInbox => 'Inbox';

  @override
  String get mailboxDrafts => 'Drafts';

  @override
  String get mailboxSent => 'Sent';

  @override
  String get mailboxArchive => 'Archive';

  @override
  String get mailboxTrash => 'Trash';

  @override
  String get mailboxJunk => 'Junk Email';

  @override
  String get virtualFlagged => 'Flagged';

  @override
  String get virtualUnread => 'Unread';
}
