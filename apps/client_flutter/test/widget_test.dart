import 'dart:typed_data';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_quill/flutter_quill.dart';
import 'package:maicenta/main.dart';
import 'package:maicenta/features/mail/account_autodiscovery.dart';
import 'package:maicenta/features/mail/account_setup_detection.dart';
import 'package:maicenta/features/mail/mail_data.dart';
import 'package:maicenta/features/mail/oauth_service.dart';
import 'package:maicenta/features/mail/safe_message_html.dart';

void main() {
  testWidgets('shows the mail workspace by default', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());

    expect(find.text('MAICENTA'), findsOneWidget);
    expect(find.byKey(const Key('title-brand-symbol')), findsOneWidget);
    expect(find.text('Posteingang'), findsWidgets);
    expect(find.text('Willkommen bei MAICENTA'), findsWidgets);
    expect(find.text('Lokaler Offline-Modus'), findsOneWidget);
    expect(find.byKey(const Key('classic-title-bar')), findsOneWidget);
    expect(find.byKey(const Key('classic-ribbon')), findsOneWidget);
    expect(find.byKey(const Key('classic-folder-pane')), findsOneWidget);
    expect(find.byKey(const Key('classic-module-bar')), findsOneWidget);
    expect(find.byKey(const Key('classic-status-bar')), findsOneWidget);

    final readingPane = find.byKey(const Key('reading-pane'));
    final readingSubject = find.descendant(
      of: readingPane,
      matching: find.text('Willkommen bei MAICENTA'),
    );
    expect(
      tester.getTopLeft(readingSubject).dy,
      lessThan(tester.getTopLeft(readingPane).dy + 60),
    );

    final folderPane = tester.getRect(
      find.byKey(const Key('classic-folder-pane')),
    );
    final moduleBar = tester.getRect(
      find.byKey(const Key('classic-module-bar')),
    );
    expect(moduleBar.left, folderPane.left);
    expect(moduleBar.right, folderPane.right);
    expect(moduleBar.bottom, folderPane.bottom);
  });

  testWidgets('localizes namespaced standard IMAP folders in English', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Work',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    const folders = [
      MailFolder(
        id: 'work.inbox',
        accountId: 'account.work',
        displayName: 'INBOX',
        role: 'inbox',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'work.drafts',
        accountId: 'account.work',
        displayName: 'INBOX.Drafts',
        role: 'drafts',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'work.sent',
        accountId: 'account.work',
        displayName: 'INBOX.Sent',
        role: 'sent',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'work.trash',
        accountId: 'account.work',
        displayName: 'INBOX.Trash',
        role: 'trash',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'work.junk',
        accountId: 'account.work',
        displayName: 'INBOX.spambucket',
        role: 'junk',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'work.templates',
        accountId: 'account.work',
        displayName: 'INBOX.Templates',
        role: 'custom',
        unreadCount: 0,
        totalCount: 0,
      ),
    ];

    await tester.pumpWidget(
      MaicentaApp(
        locale: const Locale('en'),
        mailDataSource: RecordingMailDataSource(
          configuredAccounts: const [account],
          configuredFolders: folders,
          configuredMessages: const [],
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Favorites'), findsOneWidget);
    expect(find.text('Inbox'), findsWidgets);
    expect(find.text('Drafts'), findsWidgets);
    expect(find.text('Sent'), findsWidgets);
    expect(find.text('Trash'), findsOneWidget);
    expect(find.text('Junk Email'), findsOneWidget);
    expect(find.text('Templates'), findsOneWidget);
    expect(find.text('INBOX.Drafts'), findsNothing);
    expect(find.text('INBOX.spambucket'), findsNothing);
  });

  testWidgets(
    'shows draft totals and updates unread badges when mail is opened',
    (tester) async {
      tester.view.physicalSize = const Size(1440, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      const inboxMessage = DemoMessage(
        id: 'mail.unread',
        mailboxId: 'personal.inbox',
        sender: 'Anna',
        email: 'anna@example.org',
        subject: 'Noch ungelesen',
        preview: 'Vorschau',
        body: '<p>Nachricht</p>',
        plainText: 'Nachricht',
        time: '10:00',
        unread: true,
      );
      const draft = DemoMessage(
        id: 'mail.draft',
        mailboxId: 'personal.drafts',
        sender: 'Entwurf',
        email: 'me@example.org',
        subject: 'Mein Entwurf',
        preview: 'Entwurfstext',
        body: '<p>Entwurfstext</p>',
        plainText: 'Entwurfstext',
        time: '09:00',
        draft: true,
        editableDraft: true,
      );
      final dataSource = RecordingMailDataSource(
        configuredFolders: const [
          MailFolder(
            id: 'personal.inbox',
            displayName: 'Posteingang',
            role: 'inbox',
            unreadCount: 1,
            totalCount: 1,
          ),
          MailFolder(
            id: 'personal.drafts',
            displayName: 'Entwürfe',
            role: 'drafts',
            unreadCount: 0,
            totalCount: 1,
          ),
        ],
        configuredFavoriteFolderIds: const [
          'personal.inbox',
          'personal.drafts',
        ],
        configuredMessages: const [inboxMessage, draft],
      );

      await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));

      Finder badgeIn(String key) =>
          find.descendant(of: find.byKey(Key(key)), matching: find.text('1'));

      expect(badgeIn('folder-personal.inbox'), findsOneWidget);
      expect(badgeIn('folder-personal.drafts'), findsOneWidget);
      expect(badgeIn('favorite-folder-personal.drafts'), findsOneWidget);

      await tester.tap(find.byType(MessageTile).first);
      await tester.pumpAndSettle();

      expect(dataSource.updatedMessages, hasLength(1));
      expect(dataSource.updatedMessages.single.unread, isFalse);
      expect(badgeIn('folder-personal.inbox'), findsNothing);
      expect(badgeIn('favorite-folder-personal.inbox'), findsNothing);
      expect(badgeIn('folder-personal.drafts'), findsOneWidget);

      await tester.pump(const Duration(milliseconds: 40));
      await tester.tap(find.byType(MessageTile).first);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('message-window-toggle-read')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('message-window-close')));
      await tester.pumpAndSettle();

      expect(dataSource.updatedMessages, hasLength(2));
      expect(dataSource.updatedMessages.last.unread, isTrue);
      expect(badgeIn('folder-personal.inbox'), findsOneWidget);
    },
  );

  testWidgets('qualifies duplicate favorite folder names with the account', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const firstAccount = MailAccountConfig(
      id: 'account.first',
      displayName: 'Arbeit',
      email: 'first@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'first@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'first@example.org',
    );
    const secondAccount = MailAccountConfig(
      id: 'account.second',
      displayName: 'Privat',
      email: 'second@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'second@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'second@example.org',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [firstAccount, secondAccount],
      configuredFolders: const [
        MailFolder(
          id: 'first.inbox',
          accountId: 'account.first',
          displayName: 'INBOX',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 0,
        ),
        MailFolder(
          id: 'second.inbox',
          accountId: 'account.second',
          displayName: 'INBOX',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 0,
        ),
      ],
      configuredFavoriteFolderIds: const ['first.inbox', 'second.inbox'],
      configuredMessages: const [],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));

    Tooltip tooltipFor(String folderId) => tester.widget<Tooltip>(
      find.descendant(
        of: find.byKey(Key('favorite-folder-$folderId')),
        matching: find.byType(Tooltip),
      ),
    );

    expect(tooltipFor('first.inbox').message, contains('first@example.org'));
    expect(tooltipFor('second.inbox').message, contains('second@example.org'));
  });

  testWidgets('shows pending IMAP operations in the status bar', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      MaicentaApp(
        mailDataSource: RecordingMailDataSource(pendingOperations: 2),
      ),
    );

    expect(find.textContaining('2 ausstehend'), findsOneWidget);
  });

  testWidgets('switches to dark mode and persists the profile preference', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    expect(
      Theme.of(
        tester.element(find.byKey(const Key('classic-folder-pane'))),
      ).brightness,
      Brightness.light,
    );

    await tester.tap(find.byKey(const Key('ribbon-tab-Datei')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Optionen')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('dark-mode-toggle')));
    await tester.pumpAndSettle();

    expect(dataSource.darkModeSaves, [true]);
    expect(
      Theme.of(
        tester.element(find.byKey(const Key('classic-folder-pane'))),
      ).brightness,
      Brightness.dark,
    );
  });

  testWidgets('moves a message to a mailbox by drag and drop', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.pumpAndSettle();

    final message = find.byType(MessageTile).first;
    final archive = find.byKey(
      const Key('message-drop-personal.archive-folder'),
    );
    final start = tester.getCenter(message);
    final destination = tester.getCenter(archive);
    await tester.dragFrom(start, destination - start);
    await tester.pumpAndSettle();

    expect(dataSource.updatedMessages, hasLength(1));
    expect(dataSource.updatedMessages.single.mailboxId, 'personal.archive');
  });

  testWidgets('right click opens Outlook-style mail actions', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.tapAt(
      tester.getCenter(find.byType(MessageTile).first),
      buttons: kSecondaryMouseButton,
    );
    await tester.pumpAndSettle();

    for (final action in const [
      'open',
      'reply',
      'replyAll',
      'forward',
      'toggleRead',
      'toggleFlag',
      'archive',
      'move',
      'spam',
      'delete',
    ]) {
      expect(find.byKey(Key('mail-context-$action')), findsOneWidget);
    }
  });

  testWidgets('context menu moves a mail into the junk folder', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final dataSource = RecordingMailDataSource();

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tapAt(
      tester.getCenter(find.byType(MessageTile).first),
      buttons: kSecondaryMouseButton,
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('mail-context-spam')));
    await tester.pumpAndSettle();

    expect(dataSource.updatedMessages, hasLength(1));
    expect(dataSource.updatedMessages.single.mailboxId, 'personal.junk');
  });

  testWidgets('context move submenu moves a mail to a selected folder', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final dataSource = RecordingMailDataSource();

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tapAt(
      tester.getCenter(find.byType(MessageTile).first),
      buttons: kSecondaryMouseButton,
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('mail-context-move')));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(const Key('mail-context-move-personal.archive')),
    );
    await tester.pumpAndSettle();

    expect(dataSource.updatedMessages, hasLength(1));
    expect(dataSource.updatedMessages.single.mailboxId, 'personal.archive');
  });

  testWidgets('junk context menu offers not spam and restores the inbox', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const junkMessage = DemoMessage(
      id: 'message.junk',
      mailboxId: 'personal.junk',
      sender: 'Newsletter',
      email: 'news@example.org',
      subject: 'Kein Spam',
      preview: 'Nachricht',
      body: '<p>Nachricht</p>',
      time: 'Jetzt',
    );
    final dataSource = RecordingMailDataSource(
      configuredMessages: const [junkMessage],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('folder-personal.junk')));
    await tester.pumpAndSettle();
    await tester.tapAt(
      tester.getCenter(find.byType(MessageTile).first),
      buttons: kSecondaryMouseButton,
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('mail-context-notSpam')), findsOneWidget);
    expect(find.byKey(const Key('mail-context-spam')), findsNothing);
    await tester.tap(find.byKey(const Key('mail-context-notSpam')));
    await tester.pumpAndSettle();
    expect(dataSource.updatedMessages.single.mailboxId, 'personal.inbox');
  });

  testWidgets('adds, reorders and removes favorite folders by drag and drop', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.pumpAndSettle();

    Future<void> dragBetween(Finder source, Finder target) async {
      final start = tester.getCenter(source);
      final destination = tester.getCenter(target);
      await tester.dragFrom(start, destination - start);
      await tester.pumpAndSettle();
    }

    await dragBetween(
      find.byKey(const Key('folder-personal.archive')),
      find.byKey(const Key('favorite-drop-zone')),
    );
    expect(dataSource.favoriteFolderSaves.last.last, 'personal.archive');
    expect(
      find.byKey(const Key('favorite-folder-personal.archive')),
      findsOneWidget,
    );

    await dragBetween(
      find.byKey(const Key('favorite-folder-personal.archive')),
      find.byKey(const Key('favorite-drop-before-personal.inbox')),
    );
    expect(dataSource.favoriteFolderSaves.last.first, 'personal.archive');

    await dragBetween(
      find.byKey(const Key('favorite-folder-personal.archive')),
      find.byKey(const Key('favorite-remove-account-personal')),
    );
    expect(
      dataSource.favoriteFolderSaves.last,
      isNot(contains('personal.archive')),
    );
    expect(
      find.byKey(const Key('favorite-folder-personal.archive')),
      findsNothing,
    );
  });

  testWidgets('opens a detailed classic message window on double click', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));

    await tester.tap(find.byType(MessageTile).first);
    await tester.pump(const Duration(milliseconds: 40));
    await tester.tap(find.byType(MessageTile).first);
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('classic-message-title-bar')), findsOneWidget);
    expect(find.byKey(const Key('classic-message-tabs')), findsOneWidget);
    expect(find.byKey(const Key('classic-message-ribbon')), findsOneWidget);
    expect(find.byKey(const Key('classic-message-header')), findsOneWidget);
    expect(find.byKey(const Key('message-window-html-body')), findsOneWidget);
    expect(find.byKey(const Key('message-window-reply')), findsOneWidget);
    expect(find.byKey(const Key('message-window-reply-all')), findsOneWidget);
    expect(find.byKey(const Key('message-window-forward')), findsOneWidget);

    await tester.tap(find.byKey(const Key('message-tab-Ansicht')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('message-zoom-in')));
    await tester.pump();
    expect(find.text('110 %'), findsOneWidget);

    await tester.tap(find.byKey(const Key('message-tab-Nachricht')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('message-window-toggle-flag')));
    await tester.pumpAndSettle();
    expect(dataSource.updatedMessages.last.flagged, isTrue);

    await tester.tap(find.byKey(const Key('message-window-close')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('classic-message-title-bar')), findsNothing);
  });

  testWidgets('loads older locally catalogued messages page by page', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const newest = DemoMessage(
      id: 'message.newest',
      mailboxId: 'paged.inbox',
      sender: 'Neu',
      email: 'new@example.org',
      subject: 'Neueste Nachricht',
      preview: '',
      body: '<p>Neu</p>',
      time: 'Heute',
    );
    const older = DemoMessage(
      id: 'message.older',
      mailboxId: 'paged.inbox',
      sender: 'Alt',
      email: 'old@example.org',
      subject: 'Ältere Nachricht',
      preview: '',
      body: '<p>Alt</p>',
      time: 'Gestern',
    );
    final dataSource = RecordingMailDataSource(
      configuredFolders: const [
        MailFolder(
          id: 'paged.inbox',
          displayName: 'Posteingang',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 2,
        ),
      ],
      configuredMessages: const [newest],
      configuredMailboxPage: const [older],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    expect(find.text('1 von 2 Elementen'), findsOneWidget);

    await tester.tap(find.byKey(const Key('load-more-messages')));
    await tester.pumpAndSettle();

    expect(dataSource.loadMailboxPageCalls, 1);
    expect(find.text('Ältere Nachricht'), findsWidgets);
    expect(find.text('2 Elemente'), findsOneWidget);
  });

  testWidgets('continues bounded catalogue passes until history is complete', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [
        MailAccountConfig(
          id: 'account.work',
          displayName: 'Arbeit',
          email: 'user@example.org',
          imapHost: 'imap.example.org',
          imapPort: 993,
          imapSecurity: 'tls',
          imapUsername: 'user@example.org',
          smtpHost: 'smtp.example.org',
          smtpPort: 587,
          smtpSecurity: 'starttls',
          smtpUsername: 'user@example.org',
        ),
      ],
      syncCatalogRemaining: const [500, 250, 0],
      syncDeltaMailboxes: 3,
      syncFullMailboxes: 1,
      syncQresyncMailboxes: 2,
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));

    await tester.tap(find.byKey(const Key('ribbon-tab-Senden/Empfangen')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Alle Ordner')));
    await tester.pumpAndSettle();

    expect(dataSource.synchronizeCalls, 3);
    await tester.tap(
      find.byKey(const Key('ribbon-action-Fortschritt anzeigen')),
    );
    await tester.pumpAndSettle();
    expect(
      find.textContaining('3 Ordner per Delta, 1 als Vollabgleich'),
      findsOneWidget,
    );
    expect(find.textContaining('2 Ordner nutzten QRESYNC'), findsOneWidget);
  });

  testWidgets('switches to calendar module', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.tap(find.byKey(const Key('module-Kalender')));
    await tester.pumpAndSettle();

    expect(find.text('28. Juli 2026'), findsOneWidget);
    expect(find.text('Team-Stand-up'), findsOneWidget);
  });

  testWidgets('filters messages using global search', (tester) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.enterText(find.byKey(const Key('global-search')), 'Anna');
    await tester.pump();

    expect(find.text('Anna Schneider'), findsWidgets);
    expect(find.text('Open Source Weekly'), findsNothing);
  });

  testWidgets('shows asynchronous encrypted-profile search results', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const indexedMessage = DemoMessage(
      id: 'message.indexed',
      mailboxId: 'personal.archive',
      sender: 'Archiv',
      email: 'archive@example.org',
      subject: 'Treffer aus dem verschlüsselten Index',
      preview: 'Nur im vollständigen Profil vorhanden',
      body: '<p>Quantennotiz</p>',
      plainText: 'Quantennotiz',
      time: 'Gestern',
    );
    final dataSource = RecordingMailDataSource(
      configuredSearchResults: const [indexedMessage],
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.enterText(
      find.byKey(const Key('global-search')),
      'Quantennotiz',
    );
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();

    expect(dataSource.searchCalls, 1);
    expect(dataSource.lastSearchIncludedContent, isFalse);
    expect(find.text('Treffer aus dem verschlüsselten Index'), findsWidgets);

    await tester.tap(find.byKey(const Key('include-message-content-search')));
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();

    expect(dataSource.searchCalls, 2);
    expect(dataSource.lastSearchIncludedContent, isTrue);
  });

  testWidgets('loads the body of a header-only catalogue result on demand', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const headerOnly = DemoMessage(
      id: 'message.catalogue',
      accountId: 'account.work',
      mailboxId: 'personal.inbox',
      sender: 'Anna',
      email: 'anna@example.org',
      subject: 'Katalogisierter Betreff',
      preview: '',
      body: '',
      time: 'Gestern',
    );
    const loaded = DemoMessage(
      id: 'message.catalogue',
      accountId: 'account.work',
      mailboxId: 'personal.inbox',
      sender: 'Anna',
      email: 'anna@example.org',
      subject: 'Katalogisierter Betreff',
      preview: 'Nachgeladener Inhalt',
      body: '<p>Nachgeladener Inhalt</p>',
      plainText: 'Nachgeladener Inhalt',
      time: 'Gestern',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [
        MailAccountConfig(
          id: 'account.work',
          displayName: 'Arbeit',
          email: 'user@example.org',
          imapHost: 'imap.example.org',
          imapPort: 993,
          imapSecurity: 'tls',
          imapUsername: 'user@example.org',
          smtpHost: 'smtp.example.org',
          smtpPort: 587,
          smtpSecurity: 'starttls',
          smtpUsername: 'user@example.org',
        ),
      ],
      configuredMessages: const [headerOnly],
      loadedMessage: loaded,
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.text('Katalogisierter Betreff').first);
    await tester.pumpAndSettle();

    expect(dataSource.loadMessageContentCalls, 1);
    expect(find.textContaining('Nachgeladener Inhalt'), findsWidgets);
  });

  testWidgets('removes a catalogue entry whose IMAP UID vanished', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const vanished = DemoMessage(
      id: 'message.vanished',
      accountId: 'account.work',
      mailboxId: 'personal.inbox',
      sender: 'Server',
      email: 'server@example.org',
      subject: 'Inzwischen gelöscht',
      preview: '',
      body: '',
      time: 'Gestern',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [
        MailAccountConfig(
          id: 'account.work',
          displayName: 'Arbeit',
          email: 'user@example.org',
          imapHost: 'imap.example.org',
          imapPort: 993,
          imapSecurity: 'tls',
          imapUsername: 'user@example.org',
          smtpHost: 'smtp.example.org',
          smtpPort: 587,
          smtpSecurity: 'starttls',
          smtpUsername: 'user@example.org',
        ),
      ],
      configuredMessages: const [vanished],
      messageRemovedOnLoad: true,
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.text('Inzwischen gelöscht').first);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 250));

    expect(dataSource.loadMessageContentCalls, 1);
    expect(find.text('Inzwischen gelöscht'), findsNothing);
    expect(
      find.textContaining('inzwischen auf dem IMAP-Server entfernt'),
      findsOneWidget,
    );
    expect(find.text('Nachrichteninhalt nicht verfügbar'), findsNothing);
  });

  testWidgets(
    'starts automatic IMAP synchronization for a persistent profile',
    (tester) async {
      final dataSource = RecordingMailDataSource(
        automaticSynchronization: true,
        configuredAccounts: const [
          MailAccountConfig(
            id: 'account.work',
            displayName: 'Arbeit',
            email: 'user@example.org',
            imapHost: 'imap.example.org',
            imapPort: 993,
            imapSecurity: 'tls',
            imapUsername: 'user@example.org',
            smtpHost: 'smtp.example.org',
            smtpPort: 587,
            smtpSecurity: 'starttls',
            smtpUsername: 'user@example.org',
          ),
        ],
      );

      await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
      await tester.pumpAndSettle();

      expect(dataSource.synchronizeCalls, 1);
      expect(find.text('IMAP-Synchronisierung läuft …'), findsNothing);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets('an IMAP IDLE notification triggers background synchronization', (
    tester,
  ) async {
    final dataSource = RecordingMailDataSource(
      automaticSynchronization: true,
      configuredAccounts: const [
        MailAccountConfig(
          id: 'account.work',
          displayName: 'Arbeit',
          email: 'user@example.org',
          imapHost: 'imap.example.org',
          imapPort: 993,
          imapSecurity: 'tls',
          imapUsername: 'user@example.org',
          smtpHost: 'smtp.example.org',
          smtpPort: 587,
          smtpSecurity: 'starttls',
          smtpUsername: 'user@example.org',
        ),
      ],
      configuredFolders: const [
        MailFolder(
          id: 'account.work.inbox',
          accountId: 'account.work',
          displayName: 'INBOX',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 0,
        ),
      ],
      configuredFavoriteFolderIds: const ['account.work.inbox'],
      configuredMessages: const [],
      idleOutcomes: const [
        MailboxIdleOutcome(idleSupported: true, changed: true),
        MailboxIdleOutcome(idleSupported: false, changed: false),
      ],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.pumpAndSettle();

    expect(dataSource.waitForMailboxChangeCalls, 2);
    expect(dataSource.synchronizeCalls, 2);
    expect(find.text('IMAP-Synchronisierung läuft …'), findsNothing);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('shows a readable startup error', (tester) async {
    await tester.pumpWidget(const StartupFailureApp(message: 'database error'));

    expect(find.text('MAICENTA konnte nicht gestartet werden'), findsOneWidget);
    expect(find.text('database error'), findsOneWidget);
  });

  testWidgets('opens desktop rich text compose window', (tester) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();

    expect(find.text('Neue Nachricht · HTML'), findsOneWidget);
    expect(find.text('Text formatieren'), findsOneWidget);
    expect(find.byKey(const Key('classic-compose-title-bar')), findsOneWidget);
    expect(find.byKey(const Key('classic-compose-tabs')), findsOneWidget);
    expect(find.byKey(const Key('classic-compose-ribbon')), findsOneWidget);
    expect(find.byKey(const Key('classic-compose-envelope')), findsOneWidget);
    expect(
      find.byKey(const Key('classic-compose-editor-area')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('classic-compose-status-bar')), findsOneWidget);
    expect(find.byType(QuillSimpleToolbar), findsOneWidget);
    expect(find.byType(QuillEditor), findsOneWidget);
    expect(
      tester.getSize(find.byKey(const Key('compose-send-label'))).height,
      lessThanOrEqualTo(20),
    );

    await tester.tap(find.byKey(const Key('compose-tab-Optionen')));
    await tester.pump();
    expect(find.byKey(const Key('compose-ribbon-cc')), findsOneWidget);
    expect(find.byKey(const Key('compose-ribbon-bcc')), findsOneWidget);
    expect(find.byType(QuillSimpleToolbar), findsNothing);
    await tester.tap(find.byKey(const Key('compose-ribbon-cc')));
    await tester.pump();
    expect(find.byKey(const Key('compose-cc')), findsOneWidget);

    await tester.tap(find.byKey(const Key('compose-tab-Text formatieren')));
    await tester.pump();
    expect(find.byType(QuillSimpleToolbar), findsOneWidget);

    await tester.tap(find.byKey(const Key('compose-send')));
    await tester.pump();
    expect(
      find.text('Bitte mindestens einen Empfänger angeben.'),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const Key('compose-close')));
    await tester.pumpAndSettle();
  });

  testWidgets('adds native desktop file drops as compose attachments', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();

    final target = tester.widget<DropTarget>(
      find.byKey(const Key('compose-file-drop-target')),
    );
    target.onDragEntered?.call(
      DropEventDetails(localPosition: Offset.zero, globalPosition: Offset.zero),
    );
    await tester.pump();
    expect(find.byKey(const Key('compose-file-drop-overlay')), findsOneWidget);

    final droppedFile = DropItemFile.fromData(
      Uint8List(42),
      path: '/tmp/proposal.pdf',
      name: 'proposal.pdf',
    );
    expect(await droppedFile.length(), 42);
    expect(droppedFile.path, '/tmp/proposal.pdf');
    target.onDragDone?.call(
      DropDoneDetails(
        files: [droppedFile],
        localPosition: Offset.zero,
        globalPosition: Offset.zero,
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));
    await tester.pumpAndSettle();

    expect(find.textContaining('proposal.pdf'), findsOneWidget);
    expect(
      find.byKey(const Key('classic-compose-attachments')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('compose-file-drop-overlay')), findsNothing);
  });

  testWidgets('saves a compose window directly as a local draft', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final dataSource = RecordingMailDataSource();

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('compose-subject')),
      'Schneller Entwurf',
    );
    await tester.tap(find.byKey(const Key('compose-quick-save-draft')));
    await tester.pumpAndSettle();

    expect(dataSource.savedMessages, hasLength(1));
    expect(
      dataSource.savedMessages.single.message.subject,
      'Schneller Entwurf',
    );
    expect(dataSource.savedMessages.single.message.draft, isTrue);
  });

  testWidgets('pushes a saved online draft to its IMAP account', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Arbeit',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    const folders = [
      MailFolder(
        id: 'account.work.inbox',
        accountId: 'account.work',
        displayName: 'INBOX',
        role: 'inbox',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'account.work.drafts',
        accountId: 'account.work',
        displayName: 'Drafts',
        role: 'drafts',
        unreadCount: 0,
        totalCount: 0,
      ),
      MailFolder(
        id: 'account.work.sent',
        accountId: 'account.work',
        displayName: 'Sent',
        role: 'sent',
        unreadCount: 0,
        totalCount: 0,
      ),
    ];
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [account],
      configuredFolders: folders,
      configuredMessages: const [],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('compose-subject')),
      'Synchronisierter Entwurf',
    );
    await tester.tap(find.byKey(const Key('compose-quick-save-draft')));
    await tester.pumpAndSettle();

    expect(dataSource.savedMessages.single.draft, isTrue);
    expect(dataSource.draftSynchronizeCalls, 1);
    expect(dataSource.draftSynchronizedAccountIds, ['account.work']);
    expect(
      find.text('Der Entwurf wurde lokal und im IMAP-Konto gespeichert.'),
      findsOneWidget,
    );
  });

  testWidgets('renders sanitized HTML mail in the reading pane', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.scrollUntilVisible(
      find.text('Open Source Weekly').first,
      300,
      scrollable: find.byType(Scrollable).at(1),
    );
    await tester.tap(find.text('Open Source Weekly').first);
    await tester.pumpAndSettle();

    expect(
      find.text(
        'Sichere HTML-Ansicht · Skripte und externe Inhalte sind blockiert',
      ),
      findsOneWidget,
    );
    expect(find.byKey(const Key('sanitized-html-body')), findsOneWidget);
    expect(find.textContaining('Local-first software'), findsWidgets);
  });

  test('external mail images require explicit consent', () {
    var allowed = false;
    final factory = SafeMailWidgetFactory(allowRemoteImages: () => allowed);
    const encoded =
        'https%3A%2F%2Fimages.example.org%2Fnewsletter.png%3Fid%3D42';
    const inertUrl = '$blockedRemoteImageScheme$encoded';

    expect(factory.imageProviderFromNetwork(inertUrl), isNull);
    allowed = true;
    final provider = factory.imageProviderFromNetwork(inertUrl);

    expect(provider, isA<NetworkImage>());
    expect(
      (provider! as NetworkImage).url,
      'https://images.example.org/newsletter.png?id=42',
    );
    expect(
      hasUnresolvedMessageImages('<p>Text</p><img alt="Altes Bild">'),
      isTrue,
    );
  });

  testWidgets('offers external images for an image-only remote message', (
    tester,
  ) async {
    const message = DemoMessage(
      id: 'message.remote-image',
      accountId: 'account.remote',
      mailboxId: 'account.remote.spam',
      sender: 'Newsletter',
      email: 'news@example.org',
      subject: 'Nur ein Bild',
      preview: '',
      body:
          '<img alt="Newsletter" src="maicenta-blocked-image:https%3A%2F%2Fimages.example.org%2Fmail.png">',
      time: 'Jetzt',
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ReadingPane(
            message: message,
            onReply: () {},
            onForward: () {},
            onEditDraft: () {},
            onSaveAttachment: (_) {},
            zoom: 1,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('reading-load-external-content')), findsOne);
    expect(find.text('Bilder laden'), findsOneWidget);
    expect(
      find.textContaining('möglicherweise nur aus externen Bildern'),
      findsOneWidget,
    );
  });

  testWidgets('shows and opens a locally stored attachment', (tester) async {
    MailAttachmentData? selectedAttachment;
    const attachment = MailAttachmentData(
      id: 'attachment.test',
      fileName: 'Angebot.pdf',
      contentType: 'application/pdf',
      sizeBytes: 2048,
      availableLocally: true,
    );
    const message = DemoMessage(
      id: 'message.test',
      mailboxId: 'personal.inbox',
      sender: 'Anna',
      email: 'anna@example.org',
      subject: 'Angebot',
      preview: 'Im Anhang',
      body: '<p>Im Anhang</p>',
      time: 'Jetzt',
      hasAttachment: true,
      attachments: [attachment],
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ReadingPane(
            message: message,
            onReply: () {},
            onForward: () {},
            onEditDraft: () {},
            onSaveAttachment: (value) => selectedAttachment = value,
            zoom: 1,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Angebot.pdf'), findsOneWidget);
    expect(find.text('application/pdf · 2.0 KB'), findsOneWidget);
    await tester.tap(
      find.byKey(const Key('message-attachment-attachment.test')),
    );
    expect(selectedAttachment, same(attachment));
  });

  testWidgets('keeps a compact reading pane scrollable without overflow', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(312, 462);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    const attachments = [
      MailAttachmentData(
        id: 'attachment.one',
        fileName: 'Eins.pdf',
        contentType: 'application/pdf',
        sizeBytes: 1024,
        availableLocally: true,
      ),
      MailAttachmentData(
        id: 'attachment.two',
        fileName: 'Zwei.pdf',
        contentType: 'application/pdf',
        sizeBytes: 2048,
        availableLocally: true,
      ),
      MailAttachmentData(
        id: 'attachment.three',
        fileName: 'Drei.pdf',
        contentType: 'application/pdf',
        sizeBytes: 3072,
        availableLocally: true,
      ),
      MailAttachmentData(
        id: 'attachment.four',
        fileName: 'Vier.pdf',
        contentType: 'application/pdf',
        sizeBytes: 4096,
        availableLocally: true,
      ),
      MailAttachmentData(
        id: 'attachment.five',
        fileName: 'Fünf.pdf',
        contentType: 'application/pdf',
        sizeBytes: 5120,
        availableLocally: true,
      ),
    ];
    const message = DemoMessage(
      id: 'message.compact-draft',
      mailboxId: 'personal.drafts',
      sender: 'Entwurf',
      email: 'draft@maicenta.local',
      subject: 'Kompakter Entwurf',
      preview: 'Entwurfstext',
      body: '<p>Ein Entwurf mit mehreren Anhängen.</p>',
      time: 'Jetzt',
      hasAttachment: true,
      attachments: attachments,
      draft: true,
      editableDraft: true,
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ReadingPane(
            message: message,
            onReply: () {},
            onForward: () {},
            onEditDraft: () {},
            onSaveAttachment: (_) {},
            zoom: 1,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    expect(find.byKey(const Key('reading-pane-scroll')), findsOneWidget);
    await tester.drag(
      find.byKey(const Key('reading-pane-scroll')),
      const Offset(0, -200),
    );
    await tester.pumpAndSettle();
    expect(find.text('Fünf.pdf'), findsOneWidget);
  });

  testWidgets('marks a server-backed attachment for on-demand download', (
    tester,
  ) async {
    const attachment = MailAttachmentData(
      id: 'attachment.server',
      fileName: 'Archiv.zip',
      contentType: 'application/zip',
      sizeBytes: 4096,
      availableLocally: false,
    );
    const message = DemoMessage(
      id: 'message.server',
      mailboxId: 'personal.inbox',
      sender: 'Anna',
      email: 'anna@example.org',
      subject: 'Archiv',
      preview: 'Auf dem Server',
      body: '<p>Archiv</p>',
      time: 'Jetzt',
      hasAttachment: true,
      attachments: [attachment],
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ReadingPane(
            message: message,
            onReply: () {},
            onForward: () {},
            onEditDraft: () {},
            onSaveAttachment: (_) {},
            zoom: 1,
          ),
        ),
      ),
    );

    expect(find.text('application/zip · 4.0 KB · Auf Server'), findsOneWidget);
    expect(find.byIcon(Icons.cloud_download_outlined), findsOneWidget);
  });

  testWidgets('renders a resolved inline image from memory', (tester) async {
    const message = DemoMessage(
      id: 'message.inline-image',
      mailboxId: 'personal.inbox',
      sender: 'Anna',
      email: 'anna@example.org',
      subject: 'Inline image',
      preview: 'Logo',
      body:
          '<p>Logo:</p><img alt="Logo" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=">',
      time: 'Jetzt',
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ReadingPane(
            message: message,
            onReply: () {},
            onForward: () {},
            onEditDraft: () {},
            onSaveAttachment: (_) {},
            zoom: 1,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final image = tester.widget<Image>(find.byType(Image));
    expect(image.image, isA<MemoryImage>());
  });

  testWidgets('prefills reply recipient and subject', (tester) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    await tester.tap(find.byKey(const Key('reading-reply')));
    await tester.pumpAndSettle();

    final recipient = tester.widget<TextField>(
      find.byKey(const Key('compose-to')),
    );
    final subject = tester.widget<TextField>(
      find.byKey(const Key('compose-subject')),
    );
    expect(recipient.controller?.text, 'hello@maicenta.local');
    expect(subject.controller?.text, 'Re: Willkommen bei MAICENTA');

    await tester.tap(find.byKey(const Key('compose-close')));
    await tester.pumpAndSettle();
  });

  testWidgets('archives and flags messages using ribbon actions', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('ribbon-action-Nachverfolgen')));
    await tester.pump();
    expect(dataSource.updatedMessages.single.flagged, isTrue);
    await tester.tap(find.byKey(const Key('folder-virtual.flagged')));
    await tester.pump();
    expect(find.text('Willkommen bei MAICENTA'), findsWidgets);

    await tester.tap(find.byKey(const Key('ribbon-action-Archivieren')));
    await tester.pump();
    expect(dataSource.updatedMessages.last.mailboxId, 'personal.archive');

    await tester.tap(find.byKey(const Key('folder-personal.archive')));
    await tester.pump();
    expect(find.text('Willkommen bei MAICENTA'), findsWidgets);
  });

  testWidgets('stores a composed message in the local sent folder', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('compose-to')),
      'anna@example.org',
    );
    await tester.enterText(
      find.byKey(const Key('compose-subject')),
      'Lokale Testnachricht',
    );
    await tester.tap(find.byKey(const Key('compose-send')));
    await tester.pumpAndSettle();

    expect(find.text('Gesendet'), findsWidgets);
    expect(find.text('Lokale Testnachricht'), findsWidgets);
    expect(
      find.text('Die Nachricht wurde lokal unter „Gesendet“ abgelegt.'),
      findsOneWidget,
    );
    expect(dataSource.savedMessages.single.message.mailboxId, 'personal.sent');
    expect(dataSource.savedMessages.single.draft, isFalse);
  });

  testWidgets('passes To, Cc and Bcc to the configured SMTP account', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Arbeit',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    const secondAccount = MailAccountConfig(
      id: 'account.private',
      displayName: 'Privat',
      email: 'private@example.org',
      imapHost: 'imap.private.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'private@example.org',
      smtpHost: 'smtp.private.example.org',
      smtpPort: 465,
      smtpSecurity: 'tls',
      smtpUsername: 'private@example.org',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [account, secondAccount],
      configuredFolders: const [
        MailFolder(
          id: 'account.work.inbox',
          accountId: 'account.work',
          displayName: 'Posteingang',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 0,
        ),
        MailFolder(
          id: 'account.work.sent',
          accountId: 'account.work',
          displayName: 'Gesendet',
          role: 'sent',
          unreadCount: 0,
          totalCount: 0,
        ),
        MailFolder(
          id: 'account.private.inbox',
          accountId: 'account.private',
          displayName: 'Posteingang',
          role: 'inbox',
          unreadCount: 0,
          totalCount: 0,
        ),
        MailFolder(
          id: 'account.private.sent',
          accountId: 'account.private',
          displayName: 'Gesendet',
          role: 'sent',
          unreadCount: 0,
          totalCount: 0,
        ),
      ],
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    expect(find.text('Arbeit'), findsOneWidget);
    expect(find.text('Privat'), findsOneWidget);
    expect(find.text('2 Mailkonten verbunden'), findsOneWidget);
    expect(find.text('Lokaler Online-Modus'), findsOneWidget);
    await tester.tap(find.byKey(const Key('ribbon-tab-Senden/Empfangen')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Alle Ordner')));
    await tester.pumpAndSettle();
    expect(dataSource.synchronizeCalls, 1);
    await tester.tap(find.byKey(const Key('ribbon-tab-Start')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();

    expect(find.text('Arbeit <user@example.org>'), findsOneWidget);
    await tester.tap(find.byKey(const Key('compose-from')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Privat <private@example.org>').last);
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('compose-to')),
      'anna@example.org; second@example.org',
    );
    await tester.tap(find.byKey(const Key('compose-show-cc')));
    await tester.tap(find.byKey(const Key('compose-show-bcc')));
    await tester.pump();
    await tester.enterText(
      find.byKey(const Key('compose-cc')),
      'copy@example.org',
    );
    await tester.enterText(
      find.byKey(const Key('compose-bcc')),
      'hidden@example.org',
    );
    await tester.enterText(
      find.byKey(const Key('compose-subject')),
      'SMTP-Empfängertest',
    );
    await tester.tap(find.byKey(const Key('compose-importance')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('compose-send')));
    await tester.pumpAndSettle();

    expect(dataSource.sentEnvelopes, hasLength(1));
    expect(dataSource.sentEnvelopes.single.accountId, 'account.private');
    expect(dataSource.sentEnvelopes.single.to, [
      'anna@example.org',
      'second@example.org',
    ]);
    expect(dataSource.sentEnvelopes.single.cc, ['copy@example.org']);
    expect(dataSource.sentEnvelopes.single.bcc, ['hidden@example.org']);
    expect(dataSource.sentEnvelopes.single.highImportance, isTrue);
    expect(dataSource.sentEnvelopes.single.htmlText, contains('<html>'));
    expect(
      dataSource.savedMessages.single.message.mailboxId,
      'account.private.sent',
    );
  });

  testWidgets('opens a local draft for editing and sends the same message', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Arbeit',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    const attachment = MailAttachmentData(
      id: 'attachment.draft',
      fileName: 'Plan.pdf',
      contentType: 'application/pdf',
      sizeBytes: 2048,
      availableLocally: true,
    );
    const draft = DemoMessage(
      id: 'local.draft.test',
      accountId: 'account.work',
      mailboxId: 'account.work.drafts',
      sender: 'Entwurf',
      email: 'user@example.org',
      subject: 'Bearbeitbarer Entwurf',
      preview: 'Persistierter Inhalt',
      body: '<p><strong>Persistierter Inhalt</strong></p>',
      plainText: 'Persistierter Inhalt',
      time: 'Jetzt',
      flagged: true,
      hasAttachment: true,
      attachments: [attachment],
      draft: true,
      editableDraft: true,
      draftTo: 'anna@example.org',
      draftCc: 'copy@example.org',
      editorDeltaJson:
          '[{"insert":"Persistierter Inhalt","attributes":{"bold":true}},{"insert":"\\n"}]',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [account],
      configuredFolders: const [
        MailFolder(
          id: 'account.work.drafts',
          accountId: 'account.work',
          displayName: 'Entwürfe',
          role: 'drafts',
          unreadCount: 0,
          totalCount: 1,
        ),
        MailFolder(
          id: 'account.work.sent',
          accountId: 'account.work',
          displayName: 'Gesendet',
          role: 'sent',
          unreadCount: 0,
          totalCount: 0,
        ),
      ],
      configuredMessages: const [draft],
    );

    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    expect(find.byKey(const Key('reading-edit-draft')), findsOneWidget);
    expect(find.byKey(const Key('reading-reply')), findsNothing);
    expect(
      find.byKey(const Key('ribbon-action-Entwurf bearbeiten')),
      findsOneWidget,
    );

    await tester.tap(find.byType(MessageTile).first);
    await tester.pump(const Duration(milliseconds: 40));
    await tester.tap(find.byType(MessageTile).first);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('classic-message-title-bar')), findsNothing);
    expect(find.byKey(const Key('classic-compose-title-bar')), findsOneWidget);
    expect(
      tester
          .widget<TextField>(find.byKey(const Key('compose-to')))
          .controller
          ?.text,
      'anna@example.org',
    );
    expect(
      tester
          .widget<TextField>(find.byKey(const Key('compose-cc')))
          .controller
          ?.text,
      'copy@example.org',
    );
    expect(find.text('Plan.pdf'), findsOneWidget);

    await tester.tap(find.byKey(const Key('compose-send')));
    await tester.pumpAndSettle();

    expect(dataSource.sentEnvelopes.single.storedAttachmentIds, [
      'attachment.draft',
    ]);
    expect(dataSource.savedMessages.single.message.id, 'local.draft.test');
    expect(dataSource.savedMessages.single.message.draft, isFalse);
    expect(
      dataSource.savedMessages.single.message.mailboxId,
      'account.work.sent',
    );
    expect(dataSource.savedMessages.single.retainedAttachmentIds, [
      'attachment.draft',
    ]);
  });

  testWidgets('removes a configured account after confirmation', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1400, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Arbeit',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    final dataSource = RecordingMailDataSource(
      configuredAccounts: const [account],
    );
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('ribbon-tab-Datei')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Kontoeinstellungen')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('account-row-account.work')), findsOneWidget);
    await tester.tap(find.byKey(const Key('account-delete-account.work')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('account-delete-confirm')));
    await tester.pumpAndSettle();

    expect(dataSource.deletedAccountIds, ['account.work']);
    expect(
      find.text(
        'Das Konto und seine Zugangsdaten wurden aus dem Profil entfernt.',
      ),
      findsOneWidget,
    );
  });

  testWidgets('toggles reading pane and offline mode from ribbon tabs', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(const MaicentaApp());
    expect(find.byKey(const Key('sanitized-html-body')), findsOneWidget);
    await tester.tap(find.byKey(const Key('ribbon-tab-Ansicht')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Lesebereich')));
    await tester.pump();
    expect(find.byKey(const Key('sanitized-html-body')), findsNothing);

    await tester.tap(find.byKey(const Key('ribbon-tab-Senden/Empfangen')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Online arbeiten')));
    await tester.pump();
    expect(find.text('Lokaler Online-Modus'), findsOneWidget);
  });

  testWidgets('opens password-protected profile transfer choices', (
    tester,
  ) async {
    await tester.pumpWidget(const MaicentaApp());
    await tester.tap(find.byKey(const Key('ribbon-tab-Datei')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('ribbon-action-Import/Export')));
    await tester.pumpAndSettle();

    expect(find.text('Profil sichern oder wiederherstellen'), findsOneWidget);
    expect(find.text('Importieren'), findsOneWidget);
    expect(find.text('Sicherung erstellen'), findsOneWidget);
    expect(find.textContaining('Zugangsdaten'), findsOneWidget);
  });

  testWidgets('creates and completes a local task', (tester) async {
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final dataSource = RecordingMailDataSource();
    await tester.pumpWidget(MaicentaApp(mailDataSource: dataSource));
    await tester.tap(find.byKey(const Key('module-Aufgaben')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('new-item-button')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('prompt-input')),
      'Interaktionen prüfen',
    );
    await tester.tap(find.text('Speichern'));
    await tester.pumpAndSettle();

    expect(find.text('Interaktionen prüfen'), findsOneWidget);
    await tester.tap(find.text('Interaktionen prüfen'));
    await tester.pump();
    final taskText = tester.widget<Text>(find.text('Interaktionen prüfen'));
    expect(taskText.style?.decoration, TextDecoration.lineThrough);
    expect(dataSource.savedTasks, hasLength(2));
    expect(dataSource.savedTasks.last.done, isTrue);
  });

  testWidgets('enters manual server settings when nothing is detected', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    MailAccountConfig? testedAccount;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: AccountSetupDialog(
            onDetect: (email) async => _manualOnlyDetection(email),
            onTest: (account, password) async {
              testedAccount = account;
              expect(password, 'app-password');
            },
          ),
        ),
      ),
    );
    expect(find.byKey(const Key('account-identity-step')), findsOneWidget);
    expect(find.byKey(const Key('account-password')), findsNothing);
    await _enterIdentity(tester, 'Arbeit', 'user@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('account-method-step')), findsOneWidget);
    expect(find.textContaining('kein Anbieter'), findsOneWidget);
    expect(find.byKey(const Key('account-manual-settings')), findsOneWidget);
    await tester.enterText(
      find.widgetWithText(TextField, 'Server').at(0),
      'imap.example.org',
    );
    await tester.enterText(
      find.widgetWithText(TextField, 'Server').at(1),
      'smtp.example.org',
    );
    await tester.enterText(
      find.widgetWithText(TextField, 'Passwort oder App-Passwort'),
      'app-password',
    );
    await tester.tap(find.byKey(const Key('account-test')));
    await tester.pumpAndSettle();

    expect(testedAccount?.imapHost, 'imap.example.org');
    expect(testedAccount?.imapUsername, 'user@example.org');
    expect(testedAccount?.smtpUsername, 'user@example.org');
    expect(testedAccount?.authentication, 'password');
    expect(find.text('Verbindung erfolgreich geprüft.'), findsOneWidget);
  });

  testWidgets('recommends detected IMAP servers and asks only for a password', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    MailAccountConfig? testedAccount;
    String? detectedAddress;

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: AccountSetupDialog(
            onDetect: (email) async {
              detectedAddress = email;
              return _imapDetection(email);
            },
            onTest: (account, password) async {
              testedAccount = account;
              expect(password, 'app-password');
            },
          ),
        ),
      ),
    );
    await _enterIdentity(tester, 'Arbeit', 'user@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();

    expect(detectedAddress, 'user@example.org');
    expect(
      find.byKey(const Key('account-method-imapPassword')),
      findsOneWidget,
    );
    expect(find.byKey(const Key('account-detected-servers')), findsOneWidget);
    expect(find.byKey(const Key('account-manual-settings')), findsNothing);
    expect(find.text('Empfohlen'), findsOneWidget);
    await tester.enterText(
      find.widgetWithText(TextField, 'Passwort deines E-Mail-Postfachs'),
      'app-password',
    );
    await tester.tap(find.byKey(const Key('account-test')));
    await tester.pumpAndSettle();

    expect(testedAccount?.imapHost, 'mail.example.org');
    expect(testedAccount?.imapPort, 993);
    expect(testedAccount?.smtpHost, 'mail.example.org');
    expect(testedAccount?.smtpPort, 587);
    expect(find.text('Verbindung erfolgreich geprüft.'), findsOneWidget);
  });

  testWidgets('saves a newly detected account in one step', (tester) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    AccountSetupResult? result;
    var connectionTests = 0;

    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => TextButton(
            key: const Key('open-auto-account'),
            onPressed: () async {
              result = await showDialog<AccountSetupResult>(
                context: context,
                builder: (_) => AccountSetupDialog(
                  onDetect: (email) async => _imapDetection(email),
                  onTest: (_, password) async {
                    connectionTests += 1;
                    expect(password, 'app-password');
                  },
                ),
              );
            },
            child: const Text('Öffnen'),
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('open-auto-account')));
    await tester.pumpAndSettle();
    await _enterIdentity(tester, 'Arbeit', 'user@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.widgetWithText(TextField, 'Passwort deines E-Mail-Postfachs'),
      'app-password',
    );
    await tester.tap(find.byKey(const Key('account-save')));
    await tester.pumpAndSettle();

    expect(connectionTests, 1);
    expect(result?.account.imapHost, 'mail.example.org');
    expect(result?.account.smtpPort, 587);
    expect(result?.account.provider, 'imap');
    expect(result?.password, 'app-password');
  });

  testWidgets('lets the user override the recommendation with manual servers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: AccountSetupDialog(
            onDetect: (email) async => _imapDetection(email),
            onTest: (_, _) async {},
          ),
        ),
      ),
    );
    await _enterIdentity(tester, 'Arbeit', 'user@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('account-manual-settings')), findsNothing);

    await tester.tap(find.byKey(const Key('account-method-manual')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('account-manual-settings')), findsOneWidget);
    expect(find.byKey(const Key('account-imap-host')), findsOneWidget);
    // The detected servers are prefilled so the user only adjusts what differs.
    final imapHostField = tester.widget<TextField>(
      find.descendant(
        of: find.byKey(const Key('account-imap-host')),
        matching: find.byType(TextField),
      ),
    );
    expect(imapHostField.controller?.text, 'mail.example.org');

    await tester.tap(find.byKey(const Key('account-back')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('account-identity-step')), findsOneWidget);
  });

  testWidgets('edits an existing account without re-entering its password', (
    tester,
  ) async {
    const account = MailAccountConfig(
      id: 'account.work',
      displayName: 'Arbeit',
      email: 'user@example.org',
      imapHost: 'imap.example.org',
      imapPort: 993,
      imapSecurity: 'tls',
      imapUsername: 'user@example.org',
      smtpHost: 'smtp.example.org',
      smtpPort: 587,
      smtpSecurity: 'starttls',
      smtpUsername: 'user@example.org',
    );
    AccountSetupResult? result;
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => TextButton(
            key: const Key('open-existing-account'),
            onPressed: () async {
              result = await showDialog<AccountSetupResult>(
                context: context,
                builder: (_) => AccountSetupDialog(
                  existing: account,
                  onDetect: (_) async => fail('editing must not re-detect'),
                  onTest: (_, _) async {},
                ),
              );
            },
            child: const Text('Öffnen'),
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('open-existing-account')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('account-method-step')), findsOneWidget);
    expect(find.byKey(const Key('account-back')), findsNothing);
    expect(find.byKey(const Key('account-manual-settings')), findsOneWidget);
    await tester.tap(find.byKey(const Key('account-save')));
    await tester.pumpAndSettle();

    expect(result?.account.id, 'account.work');
    expect(result?.account.imapHost, 'imap.example.org');
    expect(result?.password, isEmpty);
  });

  testWidgets('connects Exchange Online classically through OAuth and IMAP', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    MailAccountConfig? testedAccount;
    MailOAuthTokens? testedTokens;
    AccountSetupResult? result;
    final tokens = MailOAuthTokens(
      provider: MailOAuthProvider.microsoft365,
      clientId: 'public-client-id',
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      expiresAt: DateTime.utc(2030),
      tokenEndpoint:
          'https://login.microsoftonline.com/common/oauth2/v2.0/token',
      scopes: 'offline_access https://outlook.office.com/SMTP.Send',
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => TextButton(
            key: const Key('open-oauth-account'),
            onPressed: () async {
              result = await showDialog<AccountSetupResult>(
                context: context,
                builder: (_) => AccountSetupDialog(
                  onDetect: (email) async => _microsoftDetection(email),
                  onTest: (_, _) async {},
                  onAuthorizeOAuth: (provider, address) async {
                    expect(provider, MailOAuthProvider.microsoft365);
                    expect(address, 'alex@example.org');
                    return tokens;
                  },
                  onTestOAuth: (account, authorizedTokens) async {
                    testedAccount = account;
                    testedTokens = authorizedTokens;
                  },
                ),
              );
            },
            child: const Text('Öffnen'),
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('open-oauth-account')));
    await tester.pumpAndSettle();
    await _enterIdentity(tester, 'Exchange', 'alex@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Microsoft 365 registriert'), findsOneWidget);

    // The user prefers the classic protocols over the recommendation.
    await tester.tap(find.byKey(const Key('account-method-microsoftImap')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('account-oauth-login')));
    await tester.pumpAndSettle();

    expect(testedTokens, same(tokens));
    expect(testedAccount?.authentication, 'oauth2');
    expect(testedAccount?.provider, 'imap');
    expect(testedAccount?.oauthProvider, 'microsoft365');
    expect(testedAccount?.imapHost, 'outlook.office365.com');
    expect(testedAccount?.smtpHost, 'smtp.office365.com');
    // A successful sign-in finishes the setup without a second click.
    expect(result?.oauthTokens, same(tokens));
    expect(result?.account.oauthProvider, 'microsoft365');
    expect(find.byKey(const Key('account-method-step')), findsNothing);
  });

  testWidgets('recommends the Microsoft Graph API for Microsoft 365 domains', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    MailAccountConfig? testedAccount;
    AccountSetupResult? result;
    final tokens = MailOAuthTokens(
      provider: MailOAuthProvider.microsoftGraph,
      clientId: 'public-client-id',
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      expiresAt: DateTime.utc(2030),
      tokenEndpoint:
          'https://login.microsoftonline.com/common/oauth2/v2.0/token',
      scopes: 'offline_access https://graph.microsoft.com/Mail.ReadWrite',
    );

    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => TextButton(
            key: const Key('open-graph-account'),
            onPressed: () async {
              result = await showDialog<AccountSetupResult>(
                context: context,
                builder: (_) => AccountSetupDialog(
                  onDetect: (email) async => _microsoftDetection(email),
                  onTest: (_, _) async {},
                  onAuthorizeOAuth: (provider, address) async {
                    expect(provider, MailOAuthProvider.microsoftGraph);
                    return tokens;
                  },
                  onTestOAuth: (account, authorizedTokens) async {
                    testedAccount = account;
                  },
                ),
              );
            },
            child: const Text('Öffnen'),
          ),
        ),
      ),
    );
    await tester.tap(find.byKey(const Key('open-graph-account')));
    await tester.pumpAndSettle();
    await _enterIdentity(tester, 'Exchange Graph', 'alex@example.org');
    await tester.tap(find.byKey(const Key('account-continue')));
    await tester.pumpAndSettle();

    // No protocol vocabulary in the main view; the recommendation is
    // preselected and the sign-in button speaks plainly.
    expect(find.text('Mit Microsoft anmelden'), findsOneWidget);
    expect(find.byKey(const Key('account-manual-settings')), findsNothing);
    expect(find.byKey(const Key('account-password')), findsNothing);
    expect(find.text('Empfohlen'), findsOneWidget);
    // One primary action only; no duplicate "Anmelden und testen" row.
    expect(find.byKey(const Key('account-test')), findsNothing);
    expect(find.byKey(const Key('account-save')), findsNothing);

    await tester.tap(find.byKey(const Key('account-oauth-login')));
    await tester.pumpAndSettle();

    expect(testedAccount?.authentication, 'oauth2');
    expect(testedAccount?.provider, 'microsoft_graph');
    expect(testedAccount?.oauthProvider, 'microsoft_graph');
    expect(testedAccount?.imapUsername, 'alex@example.org');
    expect(result?.account.provider, 'microsoft_graph');
    expect(result?.oauthTokens, same(tokens));
  });
}

Future<void> _enterIdentity(
  WidgetTester tester,
  String name,
  String address,
) async {
  await tester.enterText(find.widgetWithText(TextField, 'Kontoname'), name);
  await tester.enterText(
    find.widgetWithText(TextField, 'E-Mail-Adresse'),
    address,
  );
}

const _detectedServers = DiscoveredMailSettings(
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

MailSetupDetection _manualOnlyDetection(String email) => MailSetupDetection(
  emailAddress: email,
  suggestions: const [
    MailSetupSuggestion(method: MailSetupMethod.manual, recommended: true),
  ],
  summary: 'Für example.org wurde kein Anbieter automatisch erkannt.',
);

MailSetupDetection _imapDetection(String email) => MailSetupDetection(
  emailAddress: email,
  suggestions: const [
    MailSetupSuggestion(
      method: MailSetupMethod.imapPassword,
      settingsCandidates: [_detectedServers],
      recommended: true,
    ),
    MailSetupSuggestion(
      method: MailSetupMethod.manual,
      settingsCandidates: [_detectedServers],
    ),
  ],
  summary: 'Servereinstellungen für example.org gefunden (DNS-SRV).',
);

MailSetupDetection _microsoftDetection(String email) => MailSetupDetection(
  emailAddress: email,
  suggestions: const [
    MailSetupSuggestion(
      method: MailSetupMethod.microsoftGraph,
      recommended: true,
    ),
    MailSetupSuggestion(method: MailSetupMethod.microsoftImap),
    MailSetupSuggestion(method: MailSetupMethod.manual),
  ],
  summary: 'Die Domain example.org ist bei Microsoft 365 registriert.',
);

class RecordingMailDataSource implements MailDataSource {
  RecordingMailDataSource({
    this.configuredAccounts = const [],
    this.configuredFolders = demoFolders,
    this.configuredFavoriteFolderIds = const [
      'personal.inbox',
      'personal.drafts',
      'personal.sent',
    ],
    this.configuredDarkModeEnabled = false,
    this.configuredMessages = demoMessages,
    this.configuredSearchResults,
    this.loadedMessage,
    this.messageRemovedOnLoad = false,
    this.automaticSynchronization = false,
    this.idleOutcomes = const [],
    this.configuredMailboxPage = const [],
    this.syncCatalogRemaining = const [],
    this.syncDeltaMailboxes = 0,
    this.syncFullMailboxes = 0,
    this.syncQresyncMailboxes = 0,
    this.pendingOperations = 0,
    this.draftSyncOutcome = const DraftSyncOutcome(synchronized: 1, pending: 0),
  });

  final List<MailAccountConfig> configuredAccounts;
  final List<MailFolder> configuredFolders;
  final List<String> configuredFavoriteFolderIds;
  final bool configuredDarkModeEnabled;
  final List<DemoMessage> configuredMessages;
  final List<DemoMessage>? configuredSearchResults;
  final DemoMessage? loadedMessage;
  final bool messageRemovedOnLoad;
  final bool automaticSynchronization;
  final List<MailboxIdleOutcome> idleOutcomes;
  final List<DemoMessage> configuredMailboxPage;
  final List<int> syncCatalogRemaining;
  final int syncDeltaMailboxes;
  final int syncFullMailboxes;
  final int syncQresyncMailboxes;
  final DraftSyncOutcome draftSyncOutcome;
  int pendingOperations;
  int synchronizeCalls = 0;
  int waitForMailboxChangeCalls = 0;
  int draftSynchronizeCalls = 0;
  final List<String> draftSynchronizedAccountIds = [];
  int searchCalls = 0;
  bool lastSearchIncludedContent = false;
  int loadMessageContentCalls = 0;
  int loadMailboxPageCalls = 0;
  final List<DemoMessage> updatedMessages = [];
  final List<List<String>> favoriteFolderSaves = [];
  final List<bool> darkModeSaves = [];
  final List<
    ({
      DemoMessage message,
      String plainText,
      String htmlText,
      List<String> attachmentPaths,
      List<String> retainedAttachmentIds,
      String draftTo,
      String draftCc,
      String draftBcc,
      String editorDeltaJson,
      bool draft,
    })
  >
  savedMessages = [];
  final List<({String attachmentId, String destinationPath})>
  exportedAttachments = [];
  final List<LocalTaskItem> savedTasks = [];
  final List<String> deletedAccountIds = [];
  final List<
    ({
      String accountId,
      List<String> to,
      List<String> cc,
      List<String> bcc,
      String subject,
      String htmlText,
      List<String> attachmentPaths,
      List<String> storedAttachmentIds,
      bool highImportance,
    })
  >
  sentEnvelopes = [];

  @override
  List<MailFolder> get folders => configuredFolders;

  @override
  List<String> get favoriteFolderIds => configuredFavoriteFolderIds;

  @override
  bool get darkModeEnabled => configuredDarkModeEnabled;

  @override
  List<DemoMessage> get messages => configuredMessages;

  @override
  List<LocalCalendarItem> get calendarEvents => demoCalendarEvents;

  @override
  List<LocalTaskItem> get tasks => demoTasks;

  @override
  List<LocalContactItem> get contacts => demoContacts;

  @override
  List<MailAccountConfig> get mailAccounts => configuredAccounts;

  @override
  int get pendingMailOperations => pendingOperations;

  @override
  bool get isPersistent => true;

  @override
  bool get automaticSynchronizationEnabled => automaticSynchronization;

  @override
  Future<MailboxIdleOutcome> waitForMailboxChange(
    String mailboxId, {
    Duration timeout = const Duration(seconds: 110),
  }) async {
    final index = waitForMailboxChangeCalls++;
    return index < idleOutcomes.length
        ? idleOutcomes[index]
        : const MailboxIdleOutcome(idleSupported: false, changed: false);
  }

  @override
  Future<DemoMessage> saveMessage(
    DemoMessage message, {
    required String plainText,
    required String htmlText,
    required List<String> attachmentPaths,
    required List<String> retainedAttachmentIds,
    required String draftTo,
    required String draftCc,
    required String draftBcc,
    required String editorDeltaJson,
    required bool draft,
  }) async {
    savedMessages.add((
      message: message,
      plainText: plainText,
      htmlText: htmlText,
      attachmentPaths: attachmentPaths,
      retainedAttachmentIds: retainedAttachmentIds,
      draftTo: draftTo,
      draftCc: draftCc,
      draftBcc: draftBcc,
      editorDeltaJson: editorDeltaJson,
      draft: draft,
    ));
    return message;
  }

  @override
  Future<void> exportAttachment(
    String attachmentId,
    String destinationPath,
  ) async {
    exportedAttachments.add((
      attachmentId: attachmentId,
      destinationPath: destinationPath,
    ));
  }

  @override
  Future<void> exportProfile(String destinationPath, String password) async {}

  @override
  Future<WorkspaceDataSnapshot> importProfile(
    String sourcePath,
    String password,
  ) async {
    return WorkspaceDataSnapshot(
      folders: folders,
      favoriteFolderIds: favoriteFolderIds,
      darkModeEnabled: darkModeEnabled,
      messages: messages,
      calendarEvents: calendarEvents,
      tasks: tasks,
      contacts: contacts,
      mailAccounts: mailAccounts,
      pendingMailOperations: pendingOperations,
    );
  }

  @override
  Future<List<DemoMessage>> searchMessages(
    String query, {
    bool includeContent = false,
  }) async {
    searchCalls += 1;
    lastSearchIncludedContent = includeContent;
    final configured = configuredSearchResults;
    if (configured != null) return configured;
    final normalized = query.toLowerCase();
    return messages
        .where(
          (message) =>
              message.sender.toLowerCase().contains(normalized) ||
              message.email.toLowerCase().contains(normalized) ||
              message.subject.toLowerCase().contains(normalized) ||
              message.draftTo.toLowerCase().contains(normalized) ||
              message.draftCc.toLowerCase().contains(normalized) ||
              message.draftBcc.toLowerCase().contains(normalized) ||
              includeContent &&
                  (message.preview.toLowerCase().contains(normalized) ||
                      message.plainText.toLowerCase().contains(normalized) ||
                      message.attachments.any(
                        (attachment) => attachment.fileName
                            .toLowerCase()
                            .contains(normalized),
                      )),
        )
        .toList(growable: false);
  }

  @override
  Future<DemoMessage?> loadMessageContent(DemoMessage message) async {
    loadMessageContentCalls += 1;
    if (messageRemovedOnLoad) return null;
    return loadedMessage ?? message;
  }

  @override
  Future<List<DemoMessage>> loadMailboxMessages(
    String mailboxId, {
    required int offset,
    int limit = 100,
  }) async {
    loadMailboxPageCalls += 1;
    return configuredMailboxPage
        .where((message) => message.mailboxId == mailboxId)
        .take(limit)
        .toList(growable: false);
  }

  @override
  Future<int> updateMessage(DemoMessage message) async {
    updatedMessages.add(message);
    return pendingOperations;
  }

  @override
  Future<void> createFolder(MailFolder folder) async {}

  @override
  Future<void> renameFolder(MailFolder folder) async {}

  @override
  Future<void> deleteFolder(String folderId, String fallbackFolderId) async {}

  @override
  Future<void> saveFavoriteFolders(List<String> folderIds) async {
    favoriteFolderSaves.add(List<String>.of(folderIds));
  }

  @override
  Future<void> saveDarkMode(bool enabled) async {
    darkModeSaves.add(enabled);
  }

  @override
  Future<void> saveCalendarEvent(LocalCalendarItem event) async {}

  @override
  Future<void> saveTask(LocalTaskItem task) async {
    savedTasks.add(task);
  }

  @override
  Future<void> saveContact(LocalContactItem contact) async {}

  @override
  Future<void> testAccount(MailAccountConfig account, String password) async {}

  @override
  Future<void> saveAccount(MailAccountConfig account, String password) async {}

  @override
  Future<void> testOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  ) async {}

  @override
  Future<void> saveOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  ) async {}

  @override
  Future<WorkspaceDataSnapshot> deleteAccount(String accountId) async {
    deletedAccountIds.add(accountId);
    return WorkspaceDataSnapshot(
      folders: folders
          .where((folder) => folder.accountId != accountId)
          .toList(growable: false),
      favoriteFolderIds: favoriteFolderIds
          .where(
            (folderId) => folders.any(
              (folder) =>
                  folder.id == folderId && folder.accountId != accountId,
            ),
          )
          .toList(growable: false),
      darkModeEnabled: darkModeEnabled,
      messages: messages
          .where((message) => message.accountId != accountId)
          .toList(growable: false),
      calendarEvents: calendarEvents,
      tasks: tasks,
      contacts: contacts,
      mailAccounts: mailAccounts
          .where((account) => account.id != accountId)
          .toList(growable: false),
      pendingMailOperations: pendingOperations,
      syncWarnings: const [],
    );
  }

  @override
  Future<WorkspaceDataSnapshot> synchronizeAccounts() async {
    synchronizeCalls += 1;
    final remainingIndex = synchronizeCalls - 1;
    return WorkspaceDataSnapshot(
      folders: folders,
      favoriteFolderIds: favoriteFolderIds,
      darkModeEnabled: darkModeEnabled,
      messages: messages,
      calendarEvents: calendarEvents,
      tasks: tasks,
      contacts: contacts,
      mailAccounts: mailAccounts,
      pendingMailOperations: pendingOperations,
      syncWarnings: const [],
      catalogMessagesRemaining: remainingIndex < syncCatalogRemaining.length
          ? syncCatalogRemaining[remainingIndex]
          : 0,
      deltaMailboxesSynchronized: syncDeltaMailboxes,
      fullMailboxesReconciled: syncFullMailboxes,
      qresyncMailboxesSynchronized: syncQresyncMailboxes,
    );
  }

  @override
  Future<DraftSyncOutcome> synchronizeDrafts(String accountId) async {
    draftSynchronizeCalls += 1;
    draftSynchronizedAccountIds.add(accountId);
    pendingOperations = draftSyncOutcome.pending;
    return draftSyncOutcome;
  }

  @override
  Future<String> sendAccountMessage({
    required String accountId,
    required List<String> to,
    required List<String> cc,
    required List<String> bcc,
    required String subject,
    required String plainText,
    required String htmlText,
    required List<String> attachmentPaths,
    required List<String> storedAttachmentIds,
    required bool highImportance,
  }) async {
    sentEnvelopes.add((
      accountId: accountId,
      to: to,
      cc: cc,
      bcc: bcc,
      subject: subject,
      htmlText: htmlText,
      attachmentPaths: attachmentPaths,
      storedAttachmentIds: storedAttachmentIds,
      highImportance: highImportance,
    ));
    return 'recorded';
  }
}
