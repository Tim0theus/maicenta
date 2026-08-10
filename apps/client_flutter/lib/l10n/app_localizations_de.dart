// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for German (`de`).
class AppLocalizationsDe extends AppLocalizations {
  AppLocalizationsDe([String locale = 'de']) : super(locale);

  @override
  String get mailModule => 'E-Mail';

  @override
  String get favorites => 'Favoriten';

  @override
  String get localArea => 'Lokaler Bereich';

  @override
  String get localDemoMode => 'Lokaler Demo-Modus';

  @override
  String mailAccountsConnected(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count Mailkonten verbunden',
      one: '1 Mailkonto verbunden',
    );
    return '$_temp0';
  }

  @override
  String get unreadEmails => 'Ungelesene E-Mails';

  @override
  String get followUp => 'Zur Nachverfolgung';

  @override
  String get accountMenu => 'Kontomenü';

  @override
  String get accountSettings => 'Kontoeinstellungen';

  @override
  String get newLocalFolder => 'Neuer lokaler Ordner';

  @override
  String get mailboxInbox => 'Posteingang';

  @override
  String get mailboxDrafts => 'Entwürfe';

  @override
  String get mailboxSent => 'Gesendet';

  @override
  String get mailboxArchive => 'Archiv';

  @override
  String get mailboxTrash => 'Papierkorb';

  @override
  String get mailboxJunk => 'Spam';

  @override
  String get virtualFlagged => 'Markiert';

  @override
  String get virtualUnread => 'Ungelesen';
}
