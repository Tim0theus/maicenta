import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_de.dart';
import 'app_localizations_en.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('de'),
    Locale('en'),
  ];

  /// No description provided for @mailModule.
  ///
  /// In de, this message translates to:
  /// **'E-Mail'**
  String get mailModule;

  /// No description provided for @favorites.
  ///
  /// In de, this message translates to:
  /// **'Favoriten'**
  String get favorites;

  /// No description provided for @localArea.
  ///
  /// In de, this message translates to:
  /// **'Lokaler Bereich'**
  String get localArea;

  /// No description provided for @localDemoMode.
  ///
  /// In de, this message translates to:
  /// **'Lokaler Demo-Modus'**
  String get localDemoMode;

  /// No description provided for @mailAccountsConnected.
  ///
  /// In de, this message translates to:
  /// **'{count, plural, one{1 Mailkonto verbunden} other{{count} Mailkonten verbunden}}'**
  String mailAccountsConnected(int count);

  /// No description provided for @unreadEmails.
  ///
  /// In de, this message translates to:
  /// **'Ungelesene E-Mails'**
  String get unreadEmails;

  /// No description provided for @followUp.
  ///
  /// In de, this message translates to:
  /// **'Zur Nachverfolgung'**
  String get followUp;

  /// No description provided for @accountMenu.
  ///
  /// In de, this message translates to:
  /// **'Kontomenü'**
  String get accountMenu;

  /// No description provided for @accountSettings.
  ///
  /// In de, this message translates to:
  /// **'Kontoeinstellungen'**
  String get accountSettings;

  /// No description provided for @newLocalFolder.
  ///
  /// In de, this message translates to:
  /// **'Neuer lokaler Ordner'**
  String get newLocalFolder;

  /// No description provided for @mailboxInbox.
  ///
  /// In de, this message translates to:
  /// **'Posteingang'**
  String get mailboxInbox;

  /// No description provided for @mailboxDrafts.
  ///
  /// In de, this message translates to:
  /// **'Entwürfe'**
  String get mailboxDrafts;

  /// No description provided for @mailboxSent.
  ///
  /// In de, this message translates to:
  /// **'Gesendet'**
  String get mailboxSent;

  /// No description provided for @mailboxArchive.
  ///
  /// In de, this message translates to:
  /// **'Archiv'**
  String get mailboxArchive;

  /// No description provided for @mailboxTrash.
  ///
  /// In de, this message translates to:
  /// **'Papierkorb'**
  String get mailboxTrash;

  /// No description provided for @mailboxJunk.
  ///
  /// In de, this message translates to:
  /// **'Spam'**
  String get mailboxJunk;

  /// No description provided for @virtualFlagged.
  ///
  /// In de, this message translates to:
  /// **'Markiert'**
  String get virtualFlagged;

  /// No description provided for @virtualUnread.
  ///
  /// In de, this message translates to:
  /// **'Ungelesen'**
  String get virtualUnread;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['de', 'en'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'de':
      return AppLocalizationsDe();
    case 'en':
      return AppLocalizationsEn();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
