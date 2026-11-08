import 'dart:async';
import 'dart:io' show File, Platform;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_quill/flutter_quill.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';
import 'package:path_provider/path_provider.dart';

import 'app_theme.dart';
import 'build_version.dart';
import 'features/about/about_dialog.dart';
import 'features/calendar/calendar_workspace.dart';
import 'features/calendar/event_editor.dart';
import 'features/calendar/recurrence.dart';
import 'features/compose/compose_window.dart';
import 'features/contacts/contact_editor.dart';
import 'features/contacts/contacts_workspace.dart';
import 'features/dav/dav_account_dialog.dart';
import 'features/mail/account_autodiscovery.dart';
import 'features/mail/account_setup_detection.dart';
import 'features/mail/folder_tree.dart';
import 'features/mail/mail_data.dart';
import 'features/mail/mailbox_labels.dart';
import 'features/mail/message_window.dart';
import 'features/mail/oauth_service.dart';
import 'features/mail/safe_message_html.dart';
import 'features/mail/sender_aliases_dialog.dart';
import 'features/reminders/reminder_dialog.dart';
import 'features/reminders/system_notifications.dart';
import 'features/tasks/task_editor.dart';
import 'features/tasks/tasks_workspace.dart';
import 'features/updates/app_updates.dart';
import 'features/updates/update_dialog.dart';
import 'features/workspace/german_dates.dart';
import 'l10n/app_localizations.dart';
import 'src/rust/frb_generated.dart';

const maicentaSymbolAsset = 'assets/branding/maicenta-symbol.png';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  registerRustLicenses();
  try {
    await RustLib.init();
    final supportDirectory = await getApplicationSupportDirectory();
    final dataSource = await RustMailDataSource.open(
      '${supportDirectory.path}/maicenta.sqlite',
    );
    final appUpdates = await AppUpdateService.open(
      // The sandboxed macOS app cannot replace itself; it only points to the
      // new DMG. Debug builds never check.
      updater: Platform.isMacOS
          ? ReleaseNoticeUpdater(
              currentVersion: maicentaVersion,
              assetSuffix: '-macos.dmg',
              supported: kReleaseMode,
            )
          : RustAppUpdater(),
      store: FileUpdatePreferencesStore(
        File('${supportDirectory.path}/update_preferences.json'),
      ),
    );
    // Taps on system notifications bring the reminder window up. The
    // notifier initializes after the first frame: on macOS that may show a
    // permission prompt, which must not delay the window.
    final notificationTaps = ValueNotifier<int>(0);
    final systemNotifier = DeferredSystemNotifier();
    runApp(
      MaicentaApp(
        mailDataSource: dataSource,
        locale: null,
        appUpdates: appUpdates,
        systemNotifier: systemNotifier,
        notificationTaps: notificationTaps,
      ),
    );
    unawaited(
      systemNotifier.attach(
        LocalSystemNotifier.open(
          onSelected: (_) => notificationTaps.value += 1,
        ),
      ),
    );
  } on Object catch (error) {
    runApp(StartupFailureApp(message: error.toString()));
  }
}

class StartupFailureApp extends StatelessWidget {
  const StartupFailureApp({super.key, required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Padding(
              padding: const EdgeInsets.all(32),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Image.asset(
                    maicentaSymbolAsset,
                    width: 82,
                    height: 72,
                    fit: BoxFit.contain,
                    filterQuality: FilterQuality.high,
                    semanticLabel: 'MAICENTA-Logo',
                  ),
                  const SizedBox(height: 14),
                  const Icon(Icons.error_outline, size: 48, color: Colors.red),
                  const SizedBox(height: 16),
                  const Text(
                    'MAICENTA konnte nicht gestartet werden',
                    style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600),
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 10),
                  const Text(
                    'Die lokale Profildatenbank oder der Rust-Core konnte nicht geöffnet werden.',
                    textAlign: TextAlign.center,
                  ),
                  const SizedBox(height: 16),
                  SelectableText(
                    message,
                    style: const TextStyle(fontSize: 11, color: Colors.black54),
                    textAlign: TextAlign.center,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class MaicentaApp extends StatefulWidget {
  const MaicentaApp({
    super.key,
    this.mailDataSource = const DemoMailDataSource(),
    this.locale = const Locale('de'),
    this.loadBuildInfo,
    this.appUpdates,
    this.systemNotifier = const NoSystemNotifier(),
    this.notificationTaps,
  });

  final MailDataSource mailDataSource;
  final Locale? locale;

  /// In-app updates; null disables update checks entirely.
  final AppUpdateService? appUpdates;

  /// Operating-system notifications for reminders.
  final SystemNotifier systemNotifier;

  /// Fires whenever the user taps a system notification.
  final Listenable? notificationTaps;

  /// Overrides how the about dialog reads version and platform, for tests.
  final Future<MaicentaBuildInfo> Function()? loadBuildInfo;

  static const primaryBlue = maicentaPrimaryBlue;

  @override
  State<MaicentaApp> createState() => _MaicentaAppState();
}

class _MaicentaAppState extends State<MaicentaApp> {
  late bool darkModeEnabled;

  @override
  void initState() {
    super.initState();
    darkModeEnabled = widget.mailDataSource.darkModeEnabled;
  }

  @override
  void didUpdateWidget(covariant MaicentaApp oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.mailDataSource != widget.mailDataSource) {
      darkModeEnabled = widget.mailDataSource.darkModeEnabled;
    }
  }

  Future<void> setDarkMode(bool enabled) async {
    if (enabled == darkModeEnabled) return;
    final previous = darkModeEnabled;
    setState(() => darkModeEnabled = enabled);
    try {
      await widget.mailDataSource.saveDarkMode(enabled);
    } on Object {
      if (mounted) setState(() => darkModeEnabled = previous);
      rethrow;
    }
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'MAICENTA',
      debugShowCheckedModeBanner: false,
      locale: widget.locale,
      localizationsDelegates: const [
        AppLocalizations.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        FlutterQuillLocalizations.delegate,
      ],
      supportedLocales: AppLocalizations.supportedLocales,
      theme: buildMaicentaTheme(Brightness.light),
      darkTheme: buildMaicentaTheme(Brightness.dark),
      themeMode: darkModeEnabled ? ThemeMode.dark : ThemeMode.light,
      home: WorkspaceShell(
        mailDataSource: widget.mailDataSource,
        darkModeEnabled: darkModeEnabled,
        onDarkModeChanged: setDarkMode,
        loadBuildInfo: widget.loadBuildInfo,
        appUpdates: widget.appUpdates,
        systemNotifier: widget.systemNotifier,
        notificationTaps: widget.notificationTaps,
      ),
    );
  }
}

enum WorkspaceModule { mail, calendar, tasks, contacts }

enum MailListFilter { all, unread, flagged }

enum MailContextAction {
  open,
  reply,
  replyAll,
  forward,
  toggleRead,
  toggleFlag,
  archive,
  delete,
  spam,
  notSpam,
  move,
}

enum MailSort { received, sender, subject }

enum ProfileTransferAction { export, import }

class RibbonCommands {
  const RibbonCommands({
    required this.newItem,
    required this.editDraft,
    required this.reply,
    required this.forward,
    required this.archive,
    required this.delete,
    required this.toggleFlag,
    required this.synchronize,
    required this.toggleOffline,
    required this.showProgress,
    required this.newFolder,
    required this.renameFolder,
    required this.deleteFolder,
    required this.markAllRead,
    required this.toggleFolderPane,
    required this.toggleReadingPane,
    required this.cycleSort,
    required this.cycleZoom,
    required this.accountSettings,
    required this.importExport,
    required this.options,
  });

  final VoidCallback newItem;
  final VoidCallback editDraft;
  final VoidCallback reply;
  final VoidCallback forward;
  final VoidCallback archive;
  final VoidCallback delete;
  final VoidCallback toggleFlag;
  final VoidCallback synchronize;
  final VoidCallback toggleOffline;
  final VoidCallback showProgress;
  final VoidCallback newFolder;
  final VoidCallback renameFolder;
  final VoidCallback deleteFolder;
  final VoidCallback markAllRead;
  final VoidCallback toggleFolderPane;
  final VoidCallback toggleReadingPane;
  final VoidCallback cycleSort;
  final VoidCallback cycleZoom;
  final VoidCallback accountSettings;
  final VoidCallback importExport;
  final VoidCallback options;
}

class WorkspaceShell extends StatefulWidget {
  const WorkspaceShell({
    super.key,
    this.mailDataSource = const DemoMailDataSource(),
    this.darkModeEnabled = false,
    this.onDarkModeChanged,
    this.loadBuildInfo,
    this.appUpdates,
    this.systemNotifier = const NoSystemNotifier(),
    this.notificationTaps,
  });

  final MailDataSource mailDataSource;
  final bool darkModeEnabled;
  final Future<void> Function(bool enabled)? onDarkModeChanged;
  final Future<MaicentaBuildInfo> Function()? loadBuildInfo;
  final AppUpdateService? appUpdates;
  final SystemNotifier systemNotifier;
  final Listenable? notificationTaps;

  @override
  State<WorkspaceShell> createState() => _WorkspaceShellState();
}

class _WorkspaceShellState extends State<WorkspaceShell>
    with WidgetsBindingObserver {
  WorkspaceModule module = WorkspaceModule.mail;
  int selectedMessage = 0;
  late String selectedFolder;
  late List<DemoMessage> messages;
  late List<MailFolder> folders;
  late List<String> favoriteFolderIds;
  late Set<String> collapsedFolderIds;
  late List<String> folderOrderIds;
  String query = '';
  List<DemoMessage>? profileSearchResults;
  bool searchInProgress = false;
  bool searchIncludesContent = false;
  final Set<String> loadingMessageContents = <String>{};
  final Set<String> markingMessagesRead = <String>{};
  bool loadingMoreMessages = false;
  bool synchronizing = false;

  /// Set while an account is being removed: no synchronization may start.
  bool removingAccount = false;

  /// Ends the catalogue continuation loop after the current pass.
  bool stopSynchronizationRequested = false;
  int catalogMessagesRemaining = 0;
  int deltaMailboxesSynchronized = 0;
  int fullMailboxesReconciled = 0;
  int qresyncMailboxesSynchronized = 0;
  Timer? searchDebounce;
  Timer? automaticSyncTimer;
  Timer? updateCheckTimer;
  bool updateCheckRunning = false;
  int idleWatcherGeneration = 0;
  int searchGeneration = 0;
  MailListFilter mailFilter = MailListFilter.all;
  MailSort mailSort = MailSort.received;
  bool offlineMode = true;

  /// Whether the user switched offline mode on. Without accounts it is on by
  /// default, which must not silence update checks.
  bool offlineModeChosen = false;
  bool showFolderPane = true;
  bool showReadingPane = true;
  bool calendarEnabled = true;
  DateTime calendarFocus = DateTime.now();
  CalendarView calendarView = CalendarView.week;
  TaskListFilter taskFilter = TaskListFilter.all;
  String? selectedTaskId;
  String? selectedContactId;
  String contactQuery = '';
  Timer? reminderTimer;

  /// Reminders the user postponed, by reminder key, until the given time.
  final Map<String, DateTime> snoozedReminders = {};

  /// Reminder keys already announced through a system notification.
  final Set<String> notifiedReminderIds = {};
  bool reminderDialogOpen = false;
  late List<DavAccountConfig> davAccounts;
  late List<WorkspaceCollection> collections;
  double readingZoom = 1;
  late List<LocalCalendarItem> calendarItems;
  late List<LocalTaskItem> tasks;
  late List<LocalContactItem> contacts;
  late List<MailAccountConfig> mailAccounts;
  late int pendingMailOperations;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    selectedFolder = widget.mailDataSource.folders.first.id;
    messages = widget.mailDataSource.messages.toList();
    folders = widget.mailDataSource.folders.toList();
    favoriteFolderIds = widget.mailDataSource.favoriteFolderIds.toList();
    collapsedFolderIds = widget.mailDataSource.collapsedFolderIds.toSet();
    folderOrderIds = widget.mailDataSource.folderOrderIds.toList();
    calendarItems = widget.mailDataSource.calendarEvents.toList();
    tasks = widget.mailDataSource.tasks.toList();
    contacts = widget.mailDataSource.contacts.toList();
    mailAccounts = widget.mailDataSource.mailAccounts.toList();
    davAccounts = widget.mailDataSource.davAccounts.toList();
    collections = widget.mailDataSource.collections.toList();
    offlineMode = mailAccounts.isEmpty && davAccounts.isEmpty;
    pendingMailOperations = widget.mailDataSource.pendingMailOperations;
    reminderTimer = Timer.periodic(
      const Duration(seconds: 30),
      (_) => unawaited(checkReminders()),
    );
    widget.notificationTaps?.addListener(onNotificationTap);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      planSystemNotifications();
      unawaited(checkReminders());
    });
    if (widget.appUpdates?.supported ?? false) {
      // Hourly ticks; the service decides whether a daily check is due.
      updateCheckTimer = Timer.periodic(
        const Duration(hours: 1),
        (_) => unawaited(checkForUpdates(automatic: true)),
      );
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(checkForUpdates(automatic: true));
      });
    }
    if (widget.mailDataSource.automaticSynchronizationEnabled) {
      automaticSyncTimer = Timer.periodic(
        const Duration(minutes: 5),
        (_) => unawaited(synchronize(automatic: true)),
      );
      WidgetsBinding.instance.addPostFrameCallback((_) async {
        if (!mounted) return;
        await synchronize(automatic: true);
        if (mounted) restartIdleWatcher();
      });
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    idleWatcherGeneration += 1;
    searchDebounce?.cancel();
    automaticSyncTimer?.cancel();
    updateCheckTimer?.cancel();
    reminderTimer?.cancel();
    widget.notificationTaps?.removeListener(onNotificationTap);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (!widget.mailDataSource.automaticSynchronizationEnabled) return;
    if (state == AppLifecycleState.resumed) {
      unawaited(resumeAutomaticSynchronization());
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.inactive ||
        state == AppLifecycleState.detached) {
      idleWatcherGeneration += 1;
    }
  }

  Future<void> resumeAutomaticSynchronization() async {
    await synchronize(automatic: true);
    if (mounted) restartIdleWatcher();
  }

  void restartIdleWatcher() {
    idleWatcherGeneration += 1;
    if (!mounted ||
        offlineMode ||
        !widget.mailDataSource.automaticSynchronizationEnabled) {
      return;
    }
    final folder = selectedFolderData;
    if (folder == null ||
        folder.accountId == 'personal' ||
        !mailAccounts.any((account) => account.id == folder.accountId)) {
      return;
    }
    final generation = idleWatcherGeneration;
    unawaited(watchMailboxWithIdle(folder.id, generation));
  }

  bool idleWatcherIsCurrent(String mailboxId, int generation) =>
      mounted &&
      !offlineMode &&
      generation == idleWatcherGeneration &&
      selectedFolder == mailboxId;

  Future<void> watchMailboxWithIdle(String mailboxId, int generation) async {
    while (idleWatcherIsCurrent(mailboxId, generation)) {
      try {
        final outcome = await widget.mailDataSource.waitForMailboxChange(
          mailboxId,
        );
        if (!idleWatcherIsCurrent(mailboxId, generation)) return;
        if (!outcome.idleSupported) return;
        if (outcome.changed) {
          while (synchronizing && idleWatcherIsCurrent(mailboxId, generation)) {
            await Future<void>.delayed(const Duration(milliseconds: 250));
          }
          if (!idleWatcherIsCurrent(mailboxId, generation)) return;
          await synchronize(automatic: true);
        }
      } on Object {
        await Future<void>.delayed(const Duration(seconds: 30));
      }
    }
  }

  void selectFolder(String folderId) {
    setState(() {
      selectedFolder = folderId;
      selectedMessage = 0;
    });
    restartIdleWatcher();
  }

  List<DemoMessage> get filteredMessages {
    final normalized = query.trim().toLowerCase();
    late Iterable<DemoMessage> candidates;
    if (normalized.isNotEmpty) {
      final searchResults = profileSearchResults;
      if (searchResults == null) {
        candidates = messages.where(
          (message) =>
              message.sender.toLowerCase().contains(normalized) ||
              message.email.toLowerCase().contains(normalized) ||
              message.subject.toLowerCase().contains(normalized) ||
              message.draftTo.toLowerCase().contains(normalized) ||
              message.draftCc.toLowerCase().contains(normalized) ||
              message.draftBcc.toLowerCase().contains(normalized) ||
              searchIncludesContent &&
                  (message.preview.toLowerCase().contains(normalized) ||
                      message.plainText.toLowerCase().contains(normalized) ||
                      message.attachments.any(
                        (attachment) => attachment.fileName
                            .toLowerCase()
                            .contains(normalized),
                      )),
        );
      } else {
        candidates = searchResults;
      }
    } else if (selectedFolder == 'virtual.flagged') {
      candidates = messages.where((message) => message.flagged);
    } else if (selectedFolder == 'virtual.unread') {
      candidates = messages.where(
        (message) => message.unread && !message.draft,
      );
    } else {
      candidates = messages.where(
        (message) => message.mailboxId == selectedFolder,
      );
    }
    candidates = switch (mailFilter) {
      MailListFilter.all => candidates,
      MailListFilter.unread => candidates.where(
        (message) => message.unread && !message.draft,
      ),
      MailListFilter.flagged => candidates.where((message) => message.flagged),
    };
    final result = candidates.toList();
    switch (mailSort) {
      case MailSort.received:
        break;
      case MailSort.sender:
        result.sort((left, right) => left.sender.compareTo(right.sender));
      case MailSort.subject:
        result.sort((left, right) => left.subject.compareTo(right.subject));
    }
    return result;
  }

  void searchWorkspace(String value, {bool includeContent = false}) {
    searchDebounce?.cancel();
    final generation = ++searchGeneration;
    final normalized = value.trim();
    setState(() {
      query = value;
      selectedMessage = 0;
      profileSearchResults = null;
      searchIncludesContent = normalized.isNotEmpty && includeContent;
      searchInProgress = normalized.isNotEmpty;
    });
    if (normalized.isEmpty) return;
    searchDebounce = Timer(const Duration(milliseconds: 250), () async {
      try {
        final results = await widget.mailDataSource.searchMessages(
          normalized,
          includeContent: includeContent,
        );
        if (!mounted || generation != searchGeneration) return;
        setState(() {
          profileSearchResults = results;
          searchInProgress = false;
          selectedMessage = 0;
        });
      } on Object catch (error) {
        if (!mounted || generation != searchGeneration) return;
        setState(() => searchInProgress = false);
        showNotice('Die lokale Profilsuche ist fehlgeschlagen: $error');
      }
    });
  }

  void includeMessageContentInSearch() {
    final activeQuery = query.trim();
    if (activeQuery.isEmpty || searchIncludesContent) return;
    searchWorkspace(query, includeContent: true);
    showNotice(
      'Zusätzlich werden lokal gespeicherte Mailtexte und Anhangsnamen durchsucht.',
    );
  }

  Future<void> selectMessage(int index) async {
    setState(() => selectedMessage = index);
    final visible = filteredMessages;
    if (index < 0 || index >= visible.length) return;
    var message = visible[index];
    if (message.unread &&
        !message.draft &&
        markingMessagesRead.add(message.id)) {
      try {
        final updated = await persistMessageUpdate(
          message.copyWith(unread: false),
        );
        if (updated != null) message = updated;
      } finally {
        markingMessagesRead.remove(message.id);
      }
      if (!mounted) return;
    }
    if (message.body.isNotEmpty ||
        message.plainText.isNotEmpty ||
        message.accountId == 'personal') {
      return;
    }
    await reloadMessageContent(message);
  }

  Future<DemoMessage?> reloadMessageContent(DemoMessage message) async {
    if (message.accountId == 'personal' ||
        loadingMessageContents.contains(message.id)) {
      return null;
    }
    if (offlineMode) {
      showNotice(
        'Der Inhalt dieser katalogisierten Nachricht ist noch nicht lokal verfügbar.',
      );
      return null;
    }
    loadingMessageContents.add(message.id);
    showNotice('Nachrichteninhalt wird sicher vom IMAP-Server geladen …');
    try {
      final loadedFromSource = await widget.mailDataSource.loadMessageContent(
        message,
      );
      if (!mounted) return null;
      if (loadedFromSource == null) {
        setState(() {
          final previous = messages
              .where((entry) => entry.id == message.id)
              .firstOrNull;
          _adjustFolderCounters(previous, null);
          messages.removeWhere((entry) => entry.id == message.id);
          profileSearchResults?.removeWhere((entry) => entry.id == message.id);
          final remaining = filteredMessages.length;
          selectedMessage = remaining == 0
              ? 0
              : selectedMessage.clamp(0, remaining - 1);
        });
        ScaffoldMessenger.of(context).clearSnackBars();
        showNotice(
          'Die Nachricht wurde inzwischen auf dem IMAP-Server entfernt und lokal aus dem Katalog gelöscht.',
        );
        return null;
      }
      final loaded = loadedFromSource.copyWith(
        mailboxId: message.mailboxId,
        unread: message.unread,
        flagged: message.flagged,
      );
      setState(() {
        final previous = messages
            .where((entry) => entry.id == loaded.id)
            .firstOrNull;
        _adjustFolderCounters(previous, loaded);
        messages = messages
            .map((entry) => entry.id == loaded.id ? loaded : entry)
            .toList();
        profileSearchResults = profileSearchResults
            ?.map((entry) => entry.id == loaded.id ? loaded : entry)
            .toList();
      });
      return loaded;
    } on Object catch (error) {
      if (!mounted) return null;
      showInformation(
        'Nachrichteninhalt nicht verfügbar',
        'Die Metadaten bleiben durchsuchbar. Der Inhalt konnte nicht vom '
            'IMAP-Server geladen werden.\n\n$error',
      );
      return null;
    } finally {
      loadingMessageContents.remove(message.id);
    }
  }

  List<MailFolder> get visibleFolders {
    return folders.map((folder) {
      final loaded = messages.where(
        (message) => message.mailboxId == folder.id,
      );
      final loadedTotal = loaded.length;
      return folder.copyWith(
        totalCount: folder.totalCount < loadedTotal
            ? loadedTotal
            : folder.totalCount,
        unreadCount: folder.unreadCount,
      );
    }).toList();
  }

  void _adjustFolderCounters(DemoMessage? before, DemoMessage? after) {
    void adjust(String mailboxId, {required int total, required int unread}) {
      final index = folders.indexWhere((folder) => folder.id == mailboxId);
      if (index < 0) return;
      final folder = folders[index];
      final totalCount = folder.totalCount + total;
      final unreadCount = folder.unreadCount + unread;
      folders[index] = folder.copyWith(
        totalCount: totalCount < 0 ? 0 : totalCount,
        unreadCount: unreadCount < 0 ? 0 : unreadCount,
      );
    }

    if (before?.mailboxId == after?.mailboxId &&
        before != null &&
        after != null) {
      adjust(
        before.mailboxId,
        total: 0,
        unread:
            (after.unread && !after.draft ? 1 : 0) -
            (before.unread && !before.draft ? 1 : 0),
      );
      return;
    }
    if (before != null) {
      adjust(
        before.mailboxId,
        total: -1,
        unread: before.unread && !before.draft ? -1 : 0,
      );
    }
    if (after != null) {
      adjust(
        after.mailboxId,
        total: 1,
        unread: after.unread && !after.draft ? 1 : 0,
      );
    }
  }

  MailFolder? get selectedFolderData {
    for (final folder in folders) {
      if (folder.id == selectedFolder) return folder;
    }
    return null;
  }

  bool get canLoadMoreMessages {
    final folder = selectedFolderData;
    if (folder == null || query.trim().isNotEmpty) return false;
    final loaded = messages
        .where((message) => message.mailboxId == folder.id)
        .length;
    return loaded < folder.totalCount;
  }

  Future<void> loadMoreMessages() async {
    final folder = selectedFolderData;
    if (folder == null || loadingMoreMessages || !canLoadMoreMessages) return;
    final offset = messages
        .where((message) => message.mailboxId == folder.id)
        .length;
    setState(() => loadingMoreMessages = true);
    try {
      final page = await widget.mailDataSource.loadMailboxMessages(
        folder.id,
        offset: offset,
      );
      if (!mounted) return;
      final knownIds = messages.map((message) => message.id).toSet();
      setState(() {
        messages.addAll(page.where((message) => knownIds.add(message.id)));
      });
    } on Object catch (error) {
      if (mounted) {
        showNotice('Ältere Nachrichten konnten nicht geladen werden: $error');
      }
    } finally {
      if (mounted) {
        setState(() => loadingMoreMessages = false);
      }
    }
  }

  void selectModule(WorkspaceModule value) {
    setState(() => module = value);
  }

  DemoMessage? get selectedMail {
    final messages = filteredMessages;
    if (messages.isEmpty) return null;
    return messages[selectedMessage.clamp(0, messages.length - 1)];
  }

  Future<void> replyToSelected() async {
    final message = selectedMail;
    if (message == null) return;
    await replyToMessage(message);
  }

  Future<void> replyAllToMessage(DemoMessage message) async {
    await replyToMessage(message, replyAll: true);
  }

  Future<void> replyToMessage(
    DemoMessage message, {
    bool replyAll = false,
  }) async {
    if (message.draft) {
      showNotice('Entwürfe werden mit „Entwurf bearbeiten“ geöffnet.');
      return;
    }
    final ownAddresses = {
      for (final account in mailAccounts)
        for (final identity in account.senderIdentities)
          identity.address.toLowerCase(),
    };
    final copiedRecipients = replyAll
        ? <String>{
                ...parseRecipientList(message.toRecipients),
                ...parseRecipientList(message.ccRecipients),
              }
              .where(
                (recipient) =>
                    recipientAddress(recipient).toLowerCase() !=
                        message.email.toLowerCase() &&
                    !ownAddresses.contains(
                      recipientAddress(recipient).toLowerCase(),
                    ),
              )
              .toList()
        : const <String>[];
    final result = await showComposeWindow(
      context,
      initialTo: message.email,
      initialCc: copiedRecipients.join('; '),
      initialSubject: prefixedSubject(message.subject, 'Re:'),
      initialBody: quotedMessage(message),
      initialAccountId: message.accountId,
      initialFromAddress: replySenderAddress(
        accountById(message.accountId),
        message,
      ),
      senders: composeSenders(mailAccounts),
    );
    await handleComposeResult(result);
  }

  Future<void> forwardSelected() async {
    final message = selectedMail;
    if (message == null) return;
    await forwardMessage(message);
  }

  Future<void> forwardMessage(DemoMessage message) async {
    if (message.draft) {
      showNotice('Ein Entwurf kann nicht weitergeleitet werden.');
      return;
    }
    final result = await showComposeWindow(
      context,
      initialSubject: prefixedSubject(message.subject, 'Fwd:'),
      initialBody: quotedMessage(message),
      initialAccountId: message.accountId,
      initialFromAddress: replySenderAddress(
        accountById(message.accountId),
        message,
      ),
      senders: composeSenders(mailAccounts),
    );
    await handleComposeResult(result);
  }

  Future<void> createMail() async {
    await handleComposeResult(
      await showComposeWindow(context, senders: composeSenders(mailAccounts)),
    );
  }

  Future<void> editSelectedDraft() async {
    final message = selectedMail;
    if (message == null || !message.draft) return;
    await editDraftMessage(message);
  }

  Future<void> editDraftMessage(DemoMessage message) async {
    if (!message.draft) return;
    if (!message.editableDraft) {
      showInformation(
        'Entwurf nicht lokal bearbeitbar',
        'Dieser IMAP-Entwurf enthält noch nicht lokal verfügbare Bestandteile. '
            'Er bleibt schreibgeschützt, damit beim Bearbeiten keine '
            'Serveranlage verloren geht.',
      );
      return;
    }
    final initialAttachments = message.attachments
        .where((attachment) => attachment.availableLocally)
        .map(
          (attachment) => ComposeAttachment(
            path: '',
            name: attachment.fileName,
            size: attachment.sizeBytes,
            storedAttachmentId: attachment.id,
          ),
        )
        .toList(growable: false);
    final result = await showComposeWindow(
      context,
      initialTo: message.draftTo,
      initialCc: message.draftCc,
      initialBcc: message.draftBcc,
      initialSubject: message.subject,
      initialBody: message.plainText,
      initialAccountId: message.accountId,
      initialFromAddress: message.email,
      initialEditorDeltaJson: message.editorDeltaJson,
      initialAttachments: initialAttachments,
      initialHighImportance: message.flagged,
      senders: composeSenders(mailAccounts),
    );
    await handleComposeResult(result, replacingDraft: message);
  }

  Future<void> handleComposeResult(
    ComposeResult? result, {
    DemoMessage? replacingDraft,
  }) async {
    if (result == null) return;
    try {
      MailAccountConfig? outgoingAccount;
      for (final account in mailAccounts) {
        if (account.id == result.accountId) {
          outgoingAccount = account;
          break;
        }
      }
      final outgoingSender = outgoingAccount?.senderFor(result.fromAddress);
      var effectiveDisposition = result.disposition;
      var sentThroughSmtp = false;
      if (result.disposition == ComposeDisposition.sent &&
          outgoingAccount != null) {
        if (offlineMode) {
          effectiveDisposition = ComposeDisposition.draft;
          showNotice(
            'Offline-Modus: Die Nachricht wird als lokaler Entwurf gespeichert.',
          );
        } else {
          final to = parseRecipientList(result.to);
          final cc = parseRecipientList(result.cc);
          final bcc = parseRecipientList(result.bcc);
          try {
            await widget.mailDataSource.sendAccountMessage(
              accountId: outgoingAccount.id,
              fromAddress: outgoingSender?.address,
              to: to,
              cc: cc,
              bcc: bcc,
              subject: result.subject,
              plainText: result.plainText,
              htmlText: result.htmlText,
              attachmentPaths: result.attachmentPaths,
              storedAttachmentIds: result.storedAttachmentIds,
              highImportance: result.highImportance,
            );
            sentThroughSmtp = true;
          } on Object catch (error) {
            if (!mounted) return;
            showInformation(
              'SMTP-Versand fehlgeschlagen',
              'Die Nachricht wurde nicht als gesendet abgelegt.\n\n$error',
            );
            return;
          }
        }
      }
      final mailboxRole = effectiveDisposition == ComposeDisposition.sent
          ? 'sent'
          : 'drafts';
      final accountId = outgoingAccount?.id ?? 'personal';
      final mailboxId = folderIdForRole(mailboxRole, accountId: accountId);
      final plainText = result.plainText.isEmpty
          ? '(Leere Nachricht)'
          : result.plainText;
      final normalized = plainText.replaceAll(RegExp(r'\s+'), ' ').trim();
      final preview = normalized.length > 120
          ? '${normalized.substring(0, 120)}…'
          : normalized;
      final message = DemoMessage(
        id:
            replacingDraft?.id ??
            'local.${DateTime.now().microsecondsSinceEpoch}',
        accountId: accountId,
        mailboxId: mailboxId,
        sender: effectiveDisposition == ComposeDisposition.sent
            ? 'Ich'
            : 'Entwurf',
        email:
            outgoingSender?.address ??
            outgoingAccount?.email ??
            'demo@maicenta.local',
        subject: result.subject,
        preview: preview,
        body: result.htmlText,
        plainText: plainText,
        time: 'Jetzt',
        flagged: result.highImportance,
        hasAttachment: result.hasAttachment,
        draft: effectiveDisposition == ComposeDisposition.draft,
        editableDraft: effectiveDisposition == ComposeDisposition.draft,
        draftTo: result.to,
        draftCc: result.cc,
        draftBcc: result.bcc,
        editorDeltaJson: result.editorDeltaJson,
      );
      late final DemoMessage savedMessage;
      try {
        savedMessage = await widget.mailDataSource.saveMessage(
          message,
          plainText: plainText,
          htmlText: result.htmlText,
          attachmentPaths: result.attachmentPaths,
          retainedAttachmentIds: result.storedAttachmentIds,
          draftTo: result.to,
          draftCc: result.cc,
          draftBcc: result.bcc,
          editorDeltaJson: result.editorDeltaJson,
          draft: effectiveDisposition == ComposeDisposition.draft,
        );
      } on Object catch (error) {
        showPersistenceError(error);
        return;
      }
      if (!mounted) return;
      final previousMessage = messages
          .where((message) => message.id == savedMessage.id)
          .firstOrNull;
      setState(() {
        _adjustFolderCounters(previousMessage, savedMessage);
        messages.removeWhere((message) => message.id == savedMessage.id);
        messages.insert(0, savedMessage);
        selectedFolder = mailboxId;
        selectedMessage = 0;
        module = WorkspaceModule.mail;
        mailFilter = MailListFilter.all;
      });
      restartIdleWatcher();
      var notice = effectiveDisposition == ComposeDisposition.sent
          ? sentThroughSmtp
                ? 'Die Nachricht wurde über SMTP gesendet und lokal abgelegt.'
                : 'Die Nachricht wurde lokal unter „Gesendet“ abgelegt.'
          : offlineMode
          ? 'Der Entwurf wurde lokal gespeichert und für IMAP vorgemerkt.'
          : 'Der Entwurf wurde lokal gespeichert.';
      final shouldSynchronizeDraft =
          outgoingAccount != null &&
          !offlineMode &&
          (effectiveDisposition == ComposeDisposition.draft ||
              replacingDraft?.draft == true);
      if (shouldSynchronizeDraft) {
        try {
          final outcome = await widget.mailDataSource.synchronizeDrafts(
            outgoingAccount.id,
          );
          if (!mounted) return;
          setState(() {
            pendingMailOperations = outcome.pending;
            if (effectiveDisposition == ComposeDisposition.draft &&
                outcome.warnings.isEmpty &&
                outcome.synchronized > 0) {
              messages = messages
                  .map(
                    (message) => message.id == savedMessage.id
                        ? message.copyWith(draftSynchronized: true)
                        : message,
                  )
                  .toList();
            }
          });
          if (outcome.warnings.isEmpty && outcome.synchronized > 0) {
            notice = effectiveDisposition == ComposeDisposition.draft
                ? 'Der Entwurf wurde lokal und im IMAP-Konto gespeichert.'
                : '$notice Der alte IMAP-Entwurf wurde entfernt.';
          } else if (outcome.warnings.isNotEmpty) {
            notice = '$notice IMAP: ${outcome.warnings.first}';
          }
        } on Object catch (error) {
          if (!mounted) return;
          notice =
              '$notice Der IMAP-Abgleich wird später erneut versucht: $error';
        }
      }
      showNotice(notice);
    } finally {
      await result.releaseSecurityScopedResources();
    }
  }

  MailAccountConfig? accountById(String accountId) {
    for (final account in mailAccounts) {
      if (account.id == accountId) return account;
    }
    return null;
  }

  String folderIdForRole(String role, {String? accountId}) {
    return folders
        .firstWhere(
          (folder) =>
              folder.role == role &&
              (accountId == null || folder.accountId == accountId),
          orElse: () => folders.firstWhere(
            (folder) => accountId == null || folder.accountId == accountId,
            orElse: () => folders.first,
          ),
        )
        .id;
  }

  Future<void> openMessageWindow(int index) async {
    final visible = filteredMessages;
    if (index < 0 || index >= visible.length) return;
    final messageId = visible[index].id;
    await selectMessage(index);
    if (!mounted) return;
    final message = messages
        .where((entry) => entry.id == messageId)
        .firstOrNull;
    if (message == null) return;
    if (message.draft && message.editableDraft) {
      await editDraftMessage(message);
      return;
    }
    await showMessageWindow(
      context,
      message: message,
      folders: folders,
      onReply: replyToMessage,
      onReplyAll: replyAllToMessage,
      onForward: forwardMessage,
      onEditDraft: editDraftMessage,
      onUpdate: persistMessageUpdate,
      onMove: moveMessageFromWindow,
      onSaveAttachment: saveAttachment,
      onReloadContent: reloadMessageContent,
    );
  }

  Future<DemoMessage?> persistMessageUpdate(DemoMessage updated) async {
    final index = messages.indexWhere((message) => message.id == updated.id);
    if (index < 0) return null;
    late final int updatedPendingOperations;
    try {
      updatedPendingOperations = await widget.mailDataSource.updateMessage(
        updated,
      );
    } on Object catch (error) {
      showPersistenceError(error);
      return null;
    }
    if (!mounted) return null;
    setState(() {
      final currentIndex = messages.indexWhere(
        (message) => message.id == updated.id,
      );
      if (currentIndex < 0) return;
      _adjustFolderCounters(messages[currentIndex], updated);
      messages[currentIndex] = updated;
      profileSearchResults = profileSearchResults
          ?.map((message) => message.id == updated.id ? updated : message)
          .toList();
      pendingMailOperations = updatedPendingOperations;
    });
    return updated;
  }

  Future<bool> moveMessageFromWindow(
    DemoMessage message,
    String mailboxId,
  ) async {
    final updated = await persistMessageUpdate(
      message.copyWith(mailboxId: mailboxId),
    );
    if (updated == null) return false;
    if (!mounted) return false;
    final folder = folders
        .where((folder) => folder.id == mailboxId)
        .firstOrNull;
    showNotice(
      '„${message.subject}“ wurde nach „${folder == null ? 'Ordner' : mailboxDisplayName(context, folder)}“ verschoben.',
    );
    return true;
  }

  Future<void> moveMessageByDrop(
    DemoMessage message,
    MailFolder targetFolder,
  ) async {
    if (message.mailboxId == targetFolder.id) return;
    if (message.accountId != targetFolder.accountId) {
      showNotice(
        'Nachrichten können derzeit nur innerhalb desselben Kontos verschoben werden.',
      );
      return;
    }
    await moveMessageFromWindow(message, targetFolder.id);
  }

  void selectMessageForContext(int index) {
    if (index < 0 || index >= filteredMessages.length) return;
    setState(() => selectedMessage = index);
  }

  Future<void> openMessageFromContext(DemoMessage message) async {
    if (!filteredMessages.any((entry) => entry.id == message.id)) return;
    var openedMessage = message;
    if (message.unread && !message.draft) {
      final updated = await persistMessageUpdate(
        message.copyWith(unread: false),
      );
      if (!mounted || updated == null) return;
      openedMessage = updated;
    }
    if (openedMessage.draft && openedMessage.editableDraft) {
      await editDraftMessage(openedMessage);
      return;
    }
    await showMessageWindow(
      context,
      message: openedMessage,
      folders: folders,
      onReply: replyToMessage,
      onReplyAll: replyAllToMessage,
      onForward: forwardMessage,
      onEditDraft: editDraftMessage,
      onUpdate: persistMessageUpdate,
      onMove: moveMessageFromWindow,
      onSaveAttachment: saveAttachment,
      onReloadContent: reloadMessageContent,
    );
  }

  Future<void> handleMailContextAction(
    DemoMessage message,
    MailContextAction action,
  ) async {
    switch (action) {
      case MailContextAction.open:
        await openMessageFromContext(message);
        return;
      case MailContextAction.reply:
        await replyToMessage(message);
        return;
      case MailContextAction.replyAll:
        await replyAllToMessage(message);
        return;
      case MailContextAction.forward:
        await forwardMessage(message);
        return;
      case MailContextAction.toggleRead:
        await persistMessageUpdate(message.copyWith(unread: !message.unread));
        return;
      case MailContextAction.toggleFlag:
        await persistMessageUpdate(message.copyWith(flagged: !message.flagged));
        return;
      case MailContextAction.archive:
        await moveMessageToRole(message, 'archive', 'archiviert');
        return;
      case MailContextAction.delete:
        await moveMessageToRole(
          message,
          'trash',
          'in den Papierkorb verschoben',
        );
        return;
      case MailContextAction.spam:
        await moveMessageToRole(message, 'junk', 'als Spam behandelt');
        return;
      case MailContextAction.notSpam:
        await moveMessageToRole(message, 'inbox', 'als „Kein Spam“ behandelt');
        return;
      case MailContextAction.move:
        return;
    }
  }

  Future<void> moveMessageToRole(
    DemoMessage message,
    String role,
    String action,
  ) async {
    final target = folders
        .where(
          (folder) =>
              folder.accountId == message.accountId && folder.role == role,
        )
        .firstOrNull;
    if (target == null) {
      showNotice('Für dieses Konto ist kein passender IMAP-Ordner verfügbar.');
      return;
    }
    if (target.id == message.mailboxId) return;
    final updated = await persistMessageUpdate(
      message.copyWith(mailboxId: target.id),
    );
    if (!mounted || updated == null) return;
    setState(() => selectedMessage = 0);
    showNotice('„${message.subject}“ wurde $action.');
  }

  Future<void> updateFavoriteFolders(List<String> orderedIds) async {
    final knownIds = folders.map((folder) => folder.id).toSet();
    final normalized = orderedIds
        .where(knownIds.contains)
        .toSet()
        .take(100)
        .toList(growable: false);
    if (listEquals(normalized, favoriteFolderIds)) return;
    final previous = favoriteFolderIds;
    setState(() => favoriteFolderIds = normalized);
    try {
      await widget.mailDataSource.saveFavoriteFolders(normalized);
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => favoriteFolderIds = previous);
      showPersistenceError(error);
    }
  }

  Future<void> updateFolderOrder(List<String> orderedIds) async {
    final knownIds = folders.map((folder) => folder.id).toSet();
    final normalized = orderedIds
        .where(knownIds.contains)
        .toSet()
        .take(2000)
        .toList(growable: false);
    if (listEquals(normalized, folderOrderIds)) return;
    final previous = folderOrderIds;
    setState(() => folderOrderIds = normalized);
    try {
      await widget.mailDataSource.saveFolderOrder(normalized);
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => folderOrderIds = previous);
      showPersistenceError(error);
    }
  }

  Future<void> toggleFolderCollapsed(String folderId) async {
    final previous = collapsedFolderIds;
    final next = Set<String>.of(previous);
    if (!next.remove(folderId)) next.add(folderId);
    setState(() => collapsedFolderIds = next);
    try {
      await widget.mailDataSource.saveCollapsedFolders(
        next.toList(growable: false),
      );
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => collapsedFolderIds = previous);
      showPersistenceError(error);
    }
  }

  Future<bool> mutateSelected(
    DemoMessage Function(DemoMessage message) update,
  ) async {
    final selected = selectedMail;
    if (selected == null) return false;
    final updated = update(selected);
    final persisted = await persistMessageUpdate(updated);
    if (persisted == null || !mounted) return false;
    setState(() {
      selectedMessage = 0;
    });
    return true;
  }

  Future<void> moveSelectedTo(String role, String action) async {
    final selected = selectedMail;
    if (selected == null) {
      showNotice('Keine Nachricht ausgewählt.');
      return;
    }
    final changed = await mutateSelected(
      (message) => message.copyWith(
        mailboxId: folderIdForRole(role, accountId: message.accountId),
      ),
    );
    if (changed) showNotice('„${selected.subject}“ wurde $action.');
  }

  Future<void> toggleSelectedFlag() async {
    final selected = selectedMail;
    if (selected == null) return;
    final changed = await mutateSelected(
      (message) => message.copyWith(flagged: !message.flagged),
    );
    if (changed) {
      showNotice(
        selected.flagged ? 'Markierung entfernt.' : 'Nachricht markiert.',
      );
    }
  }

  Future<void> markAllRead() async {
    final visibleIds = filteredMessages.map((message) => message.id).toSet();
    final updates = filteredMessages
        .where((message) => message.unread && !message.draft)
        .map((message) => message.copyWith(unread: false))
        .toList();
    var updatedPendingOperations = pendingMailOperations;
    try {
      for (final message in updates) {
        updatedPendingOperations = await widget.mailDataSource.updateMessage(
          message,
        );
      }
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      final updatesById = {for (final message in updates) message.id: message};
      for (final message in messages) {
        final updated = updatesById[message.id];
        if (updated != null) _adjustFolderCounters(message, updated);
      }
      messages = messages
          .map(
            (message) => visibleIds.contains(message.id)
                ? message.copyWith(unread: false)
                : message,
          )
          .toList();
      profileSearchResults = profileSearchResults
          ?.map(
            (message) => visibleIds.contains(message.id)
                ? message.copyWith(unread: false)
                : message,
          )
          .toList();
      pendingMailOperations = updatedPendingOperations;
      selectedMessage = 0;
    });
    showNotice('${updates.length} Nachrichten wurden als gelesen markiert.');
  }

  Future<void> synchronize({bool automatic = false}) async {
    if (removingAccount) {
      if (!automatic) {
        showNotice(
          'Ein Konto wird gerade entfernt. Danach kann erneut synchronisiert werden.',
        );
      }
      return;
    }
    if (synchronizing) {
      if (!automatic) {
        showNotice('Die Synchronisierung läuft bereits.');
      }
      return;
    }
    if (offlineMode) {
      if (!automatic) {
        showNotice(
          'Offline-Modus aktiv: Der lokale Datenbestand ist verfügbar.',
        );
      }
      return;
    }
    if (mailAccounts.isEmpty && davAccounts.isEmpty) {
      if (!automatic) {
        showNotice('Bitte zuerst ein Konto einrichten.');
        await showAccountSettings();
      }
      return;
    }
    setState(() => synchronizing = true);
    if (!automatic) showNotice('Synchronisierung läuft …');
    final warnings = <String>{};
    int? previousRemaining;
    String? davSummary;
    try {
      while (mailAccounts.isNotEmpty &&
          mounted &&
          !offlineMode &&
          !stopSynchronizationRequested) {
        final snapshot = await widget.mailDataSource.synchronizeAccounts();
        if (!mounted) return;
        warnings.addAll(snapshot.syncWarnings);
        replaceWorkspace(snapshot);
        final remaining = snapshot.catalogMessagesRemaining;
        if (remaining == 0) break;
        if (previousRemaining != null && remaining >= previousRemaining) {
          warnings.add(
            'Der Nachrichtenkatalog macht momentan keinen weiteren Fortschritt. '
            'Er wird beim nächsten Abgleich erneut versucht.',
          );
          break;
        }
        previousRemaining = remaining;
        if (!automatic) {
          showNotice(
            'Der Suchkatalog wird im Hintergrund fortgesetzt: noch $remaining Nachrichten.',
          );
        }
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      if (davAccounts.isNotEmpty && mounted && !offlineMode) {
        final outcome = await widget.mailDataSource.synchronizeDavAccounts();
        if (!mounted) return;
        warnings.addAll(outcome.warnings);
        replaceWorkspace(outcome.snapshot);
        davSummary =
            'Kalender und Kontakte: ${outcome.uploaded} gesendet, '
            '${outcome.downloaded} empfangen, ${outcome.removed} entfernt.';
      }
      if (!mounted) return;
      if (!automatic && warnings.isEmpty && catalogMessagesRemaining == 0) {
        final catalogued = folders.fold<int>(
          0,
          (count, folder) => count + folder.totalCount,
        );
        showNotice(
          [
            if (mailAccounts.isNotEmpty)
              '$catalogued Nachrichten sind lokal katalogisiert '
                  '($deltaMailboxesSynchronized Delta-, '
                  '$fullMailboxesReconciled Vollabgleich, '
                  '$qresyncMailboxesSynchronized QRESYNC).',
            ?davSummary,
          ].join(' '),
        );
      } else if (!automatic && warnings.isNotEmpty) {
        showInformation(
          'Synchronisierung abgeschlossen',
          'Die verfügbaren Konten wurden aktualisiert. Hinweise zu '
              'Suchkatalog, Inhalten oder einzelnen Konten:\n\n'
              '${warnings.join('\n')}',
        );
      }
    } on Object catch (error) {
      if (!mounted) return;
      if (!automatic) {
        showInformation(
          'Synchronisierung fehlgeschlagen',
          'Der lokale Datenbestand bleibt verfügbar.\n\n$error',
        );
      }
    } finally {
      if (mounted) setState(() => synchronizing = false);
    }
  }

  void toggleOffline() {
    setState(() {
      offlineMode = !offlineMode;
      offlineModeChosen = offlineMode;
    });
    restartIdleWatcher();
    showNotice(
      offlineMode ? 'Offline-Modus aktiviert.' : 'Online-Modus vorbereitet.',
    );
  }

  void cycleSort() {
    setState(() {
      mailSort = MailSort.values[(mailSort.index + 1) % MailSort.values.length];
      selectedMessage = 0;
    });
    final label = switch (mailSort) {
      MailSort.received => 'Empfangsreihenfolge',
      MailSort.sender => 'Absender',
      MailSort.subject => 'Betreff',
    };
    showNotice('Sortierung: $label');
  }

  void cycleZoom() {
    setState(() {
      readingZoom = switch (readingZoom) {
        1 => 1.15,
        1.15 => 1.3,
        _ => 1,
      };
    });
    showNotice('Lesezoom: ${(readingZoom * 100).round()} %');
  }

  Future<void> createFolder() async {
    final name = await showTextPrompt(
      context,
      title: 'Neuer Ordner',
      label: 'Ordnername',
    );
    if (name == null || name.trim().isEmpty) return;
    final folder = MailFolder(
      id: 'local.folder.${DateTime.now().microsecondsSinceEpoch}',
      displayName: name.trim(),
      role: 'custom',
      unreadCount: 0,
      totalCount: 0,
    );
    try {
      await widget.mailDataSource.createFolder(folder);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      folders.add(folder);
      selectedFolder = folder.id;
      selectedMessage = 0;
    });
    restartIdleWatcher();
  }

  Future<void> renameFolder() async {
    final index = folders.indexWhere((folder) => folder.id == selectedFolder);
    if (index < 0) {
      showNotice('Virtuelle Favoriten können nicht umbenannt werden.');
      return;
    }
    if (folders[index].accountId != 'personal' ||
        folders[index].role != 'custom') {
      showNotice('Nur selbst erstellte lokale Ordner können umbenannt werden.');
      return;
    }
    final name = await showTextPrompt(
      context,
      title: 'Ordner umbenennen',
      label: 'Neuer Name',
      initialValue: folders[index].displayName,
    );
    if (name == null || name.trim().isEmpty) return;
    final renamed = folders[index].copyWith(displayName: name.trim());
    try {
      await widget.mailDataSource.renameFolder(renamed);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() => folders[index] = renamed);
  }

  Future<void> deleteFolder() async {
    final index = folders.indexWhere((folder) => folder.id == selectedFolder);
    if (index < 0 ||
        folders[index].accountId != 'personal' ||
        folders[index].role != 'custom') {
      showNotice('Nur selbst erstellte lokale Ordner können gelöscht werden.');
      return;
    }
    final inboxId = folderIdForRole('inbox');
    final deletedId = folders[index].id;
    try {
      await widget.mailDataSource.deleteFolder(deletedId, inboxId);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      messages = messages
          .map(
            (message) => message.mailboxId == deletedId
                ? message.copyWith(mailboxId: inboxId)
                : message,
          )
          .toList();
      profileSearchResults = profileSearchResults
          ?.map(
            (message) => message.mailboxId == deletedId
                ? message.copyWith(mailboxId: inboxId)
                : message,
          )
          .toList();
      folders.removeAt(index);
      selectedFolder = inboxId;
      selectedMessage = 0;
    });
    restartIdleWatcher();
    showNotice(
      'Der lokale Ordner wurde gelöscht; enthaltene Nachrichten liegen im Posteingang.',
    );
  }

  Future<void> handleNewItem() async {
    switch (module) {
      case WorkspaceModule.mail:
        await createMail();
      case WorkspaceModule.calendar:
        await openEvent(null, initialStart: defaultEventStart());
      case WorkspaceModule.tasks:
        await openTask(null);
      case WorkspaceModule.contacts:
        await openContact(null);
    }
  }

  // ---------------------------------------------------------------------
  // Calendar

  /// Start proposed for a new event: the focused day at the next full hour
  /// when it is today, otherwise 09:00.
  DateTime defaultEventStart() {
    final now = DateTime.now();
    if (isSameDay(calendarFocus, now)) {
      return DateTime(now.year, now.month, now.day, now.hour + 1);
    }
    return DateTime(
      calendarFocus.year,
      calendarFocus.month,
      calendarFocus.day,
      9,
    );
  }

  /// Synchronized calendars that may hold events.
  List<WorkspaceCollection> get eventCalendars =>
      collections.where((collection) => collection.holdsEvents).toList();

  /// Synchronized lists that may hold tasks.
  List<WorkspaceCollection> get taskLists =>
      collections.where((collection) => collection.holdsTasks).toList();

  /// Synchronized address books.
  List<WorkspaceCollection> get addressBooks =>
      collections.where((collection) => collection.holdsContacts).toList();

  bool collectionEnabled(String? collectionId) {
    if (collectionId == null) return true;
    return collections.any(
      (collection) => collection.id == collectionId && collection.enabled,
    );
  }

  /// Events of the personal calendar and every switched-on calendar.
  List<LocalCalendarItem> get visibleCalendarEvents => calendarItems
      .where(
        (event) => event.collectionId == null
            ? calendarEnabled
            : collectionEnabled(event.collectionId),
      )
      .toList();

  List<LocalTaskItem> get visibleTasks =>
      tasks.where((task) => collectionEnabled(task.collectionId)).toList();

  List<LocalContactItem> get visibleContacts => contacts
      .where((contact) => collectionEnabled(contact.collectionId))
      .toList();

  Color colorForEvent(LocalCalendarItem event) {
    final id = event.collectionId;
    if (id == null) return MaicentaApp.primaryBlue;
    final collection = collections.cast<WorkspaceCollection?>().firstWhere(
      (item) => item!.id == id,
      orElse: () => null,
    );
    return parseCssColor(collection?.color) ?? MaicentaApp.primaryBlue;
  }

  Future<void> toggleCollection(
    WorkspaceCollection collection,
    bool enabled,
  ) async {
    try {
      await widget.mailDataSource.saveCollectionEnabled(collection.id, enabled);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = collections.indexWhere((item) => item.id == collection.id);
      if (index >= 0) {
        collections[index] = collection.copyWith(enabled: enabled);
      }
    });
  }

  Future<void> openEvent(
    LocalCalendarItem? event, {
    DateTime? initialStart,
    bool initialAllDay = false,
    DateTime? occurrenceDate,
  }) async {
    final result = await showEventEditor(
      context,
      event: event,
      initialStart: initialStart,
      initialAllDay: initialAllDay,
      occurrenceDate: occurrenceDate,
      calendars: eventCalendars,
    );
    if (result == null || !mounted) return;
    switch (result.action) {
      case EditorAction.save:
        await saveEvent(result.event);
      case EditorAction.delete:
        await deleteEvent(result.event);
    }
  }

  Future<void> saveEvent(LocalCalendarItem event) async {
    try {
      await widget.mailDataSource.saveCalendarEvent(event);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = calendarItems.indexWhere((item) => item.id == event.id);
      final previousStart = index < 0 ? null : calendarItems[index].startsAt;
      if (index < 0) {
        calendarItems.add(event);
      } else {
        calendarItems[index] = event;
      }
      calendarItems.sort((a, b) => a.startsAt.compareTo(b.startsAt));
      // Jump to a new or moved event; keep the view when only details
      // (or a single occurrence of a series) changed.
      if (previousStart != event.startsAt) calendarFocus = event.startsAt;
      snoozedReminders.removeWhere((key, _) => key.startsWith(event.id));
    });
    planSystemNotifications();
    showNotice('Der Termin wurde gespeichert.');
  }

  Future<void> deleteEvent(LocalCalendarItem event) async {
    try {
      await widget.mailDataSource.deleteCalendarEvent(event.id);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      calendarItems.removeWhere((item) => item.id == event.id);
      snoozedReminders.removeWhere((key, _) => key.startsWith(event.id));
    });
    planSystemNotifications();
    showNotice('Der Termin wurde gelöscht.');
  }

  // ---------------------------------------------------------------------
  // Tasks

  Future<void> openTask(LocalTaskItem? task, {String initialTitle = ''}) async {
    final result = await showTaskEditor(
      context,
      task: task,
      initialTitle: initialTitle,
      lists: taskLists,
    );
    if (result == null || !mounted) return;
    switch (result.action) {
      case EditorAction.save:
        await saveTask(result.task);
      case EditorAction.delete:
        await deleteTask(result.task);
    }
  }

  Future<void> saveTask(LocalTaskItem task) async {
    try {
      await widget.mailDataSource.saveTask(task);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = tasks.indexWhere((item) => item.id == task.id);
      if (index < 0) {
        tasks.insert(0, task);
      } else {
        tasks[index] = task;
      }
      selectedTaskId = task.id;
      snoozedReminders.remove(task.id);
    });
    planSystemNotifications();
  }

  Future<void> quickAddTask(String title) async {
    final now = DateTime.now();
    await saveTask(
      LocalTaskItem(
        id: 'local.task.${now.microsecondsSinceEpoch}',
        title: title,
        dueAt: taskFilter == TaskListFilter.today ? dateOnly(now) : null,
      ),
    );
    if (mounted) showNotice('Die Aufgabe wurde angelegt.');
  }

  Future<void> toggleTask(LocalTaskItem task) async {
    await saveTask(task.withDone(!task.done));
  }

  Future<void> deleteTask(LocalTaskItem task) async {
    try {
      await widget.mailDataSource.deleteTask(task.id);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      tasks.removeWhere((item) => item.id == task.id);
      if (selectedTaskId == task.id) selectedTaskId = null;
      snoozedReminders.remove(task.id);
    });
    planSystemNotifications();
    showNotice('Die Aufgabe wurde gelöscht.');
  }

  Future<void> confirmDeleteTask(LocalTaskItem task) async {
    final confirmed = await confirmDeletion(
      'Aufgabe löschen?',
      '„${task.title}“ wird endgültig entfernt.',
    );
    if (confirmed) await deleteTask(task);
  }

  // ---------------------------------------------------------------------
  // Contacts

  Future<void> openContact(LocalContactItem? contact) async {
    final result = await showContactEditor(
      context,
      contact: contact,
      addressBooks: addressBooks,
    );
    if (result == null || !mounted) return;
    switch (result.action) {
      case EditorAction.save:
        await saveContact(result.contact);
      case EditorAction.delete:
        await deleteContact(result.contact);
    }
  }

  Future<void> saveContact(LocalContactItem contact) async {
    try {
      await widget.mailDataSource.saveContact(contact);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = contacts.indexWhere((item) => item.id == contact.id);
      if (index < 0) {
        contacts.add(contact);
      } else {
        contacts[index] = contact;
      }
      selectedContactId = contact.id;
    });
    showNotice('Der Kontakt wurde gespeichert.');
  }

  Future<void> deleteContact(LocalContactItem contact) async {
    try {
      await widget.mailDataSource.deleteContact(contact.id);
    } on Object catch (error) {
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      contacts.removeWhere((item) => item.id == contact.id);
      if (selectedContactId == contact.id) selectedContactId = null;
    });
    showNotice('Der Kontakt wurde gelöscht.');
  }

  Future<void> confirmDeleteContact(LocalContactItem contact) async {
    final confirmed = await confirmDeletion(
      'Kontakt löschen?',
      '„${contact.name}“ wird endgültig entfernt.',
    );
    if (confirmed) await deleteContact(contact);
  }

  Future<void> composeToContact(
    LocalContactItem contact,
    String address,
  ) async {
    final result = await showComposeWindow(
      context,
      initialTo: '${contact.name} <$address>',
      senders: composeSenders(mailAccounts),
    );
    await handleComposeResult(result);
  }

  Future<bool> confirmDeletion(String title, String message) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(title),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            key: const Key('confirm-delete'),
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Löschen'),
          ),
        ],
      ),
    );
    return confirmed ?? false;
  }

  // ---------------------------------------------------------------------
  // Reminders

  /// Reminders that are due now and neither closed nor postponed.
  /// How far back an unanswered reminder is still shown; older ones are
  /// considered stale (for example events downloaded from a shared calendar).
  static const reminderLookback = Duration(days: 7);

  /// How far ahead reminders are handed to the operating system.
  static const reminderHorizon = Duration(days: 14);

  static String reminderKey(
    LocalCalendarItem event,
    EventOccurrence occurrence,
  ) => event.recurring ? '${event.id}#${occurrence.isoDate}' : event.id;

  String eventReminderDetail(EventOccurrence occurrence) => [
    if (occurrence.allDay)
      'Ganztägig am ${formatDate(occurrence.startsAt)}'
    else
      'Beginn: ${formatDateTime(occurrence.startsAt)}',
    if (occurrence.event.location case final location? when location.isNotEmpty)
      location,
  ].join(' · ');

  String taskReminderDetail(LocalTaskItem task) => task.dueAt == null
      ? 'Aufgabe ohne Fälligkeitsdatum'
      : 'Fällig am ${formatDate(task.dueAt!)}';

  /// Reminders that are due now and neither closed nor postponed.
  List<ReminderEntry> dueReminders(DateTime now) {
    bool postponed(String id) {
      final until = snoozedReminders[id];
      return until != null && until.isAfter(now);
    }

    final entries = <ReminderEntry>[];
    final windowStart = now.subtract(reminderLookback);
    final windowEnd = now.add(const Duration(days: 3));
    for (final event in calendarItems) {
      if (event.reminderMinutesBefore == null) continue;
      for (final occurrence in occurrencesBetween(
        event,
        windowStart,
        windowEnd,
      )) {
        final at = occurrence.reminderAt;
        if (at == null || at.isAfter(now) || occurrence.reminderDismissed) {
          continue;
        }
        final key = reminderKey(event, occurrence);
        if (postponed(key)) continue;
        entries.add(
          ReminderEntry(
            id: key,
            itemId: event.id,
            kind: ReminderKind.event,
            title: event.title,
            detail: eventReminderDetail(occurrence),
            dueAt: occurrence.startsAt,
            occurrenceStart: occurrence.startsAt,
          ),
        );
      }
    }
    for (final task in tasks) {
      final at = task.reminderAt;
      if (at == null ||
          task.done ||
          task.reminderDismissed ||
          at.isAfter(now) ||
          postponed(task.id)) {
        continue;
      }
      entries.add(
        ReminderEntry(
          id: task.id,
          itemId: task.id,
          kind: ReminderKind.task,
          title: task.title,
          detail: taskReminderDetail(task),
          dueAt: at,
        ),
      );
    }
    entries.sort((a, b) => a.dueAt.compareTo(b.dueAt));
    return entries;
  }

  /// Reminders that will become due within [reminderHorizon], including
  /// postponed ones at their new time.
  List<PlannedNotification> upcomingReminders(DateTime now) {
    final horizon = now.add(reminderHorizon);
    final planned = <PlannedNotification>[];
    for (final event in calendarItems) {
      if (event.reminderMinutesBefore == null) continue;
      for (final occurrence in occurrencesBetween(
        event,
        now,
        horizon.add(const Duration(days: 3)),
      )) {
        final at = occurrence.reminderAt;
        if (at == null || occurrence.reminderDismissed) continue;
        final key = reminderKey(event, occurrence);
        final due = snoozedReminders[key] ?? at;
        if (!due.isAfter(now) || due.isAfter(horizon)) continue;
        planned.add(
          PlannedNotification(
            id: key,
            title: event.title,
            body: eventReminderDetail(occurrence),
            at: due,
          ),
        );
      }
    }
    for (final task in tasks) {
      final at = task.reminderAt;
      if (at == null || task.done || task.reminderDismissed) continue;
      final due = snoozedReminders[task.id] ?? at;
      if (!due.isAfter(now) || due.isAfter(horizon)) continue;
      planned.add(
        PlannedNotification(
          id: task.id,
          title: task.title,
          body: taskReminderDetail(task),
          at: due,
        ),
      );
    }
    return planned;
  }

  /// Hands the upcoming reminders to the operating system.
  void planSystemNotifications() {
    unawaited(
      widget.systemNotifier.schedule(upcomingReminders(DateTime.now())),
    );
  }

  void onNotificationTap() {
    if (!mounted) return;
    unawaited(checkReminders());
  }

  Future<void> checkReminders() async {
    if (!mounted || reminderDialogOpen) return;
    final entries = dueReminders(DateTime.now());
    if (entries.isEmpty) return;
    for (final entry in entries) {
      if (notifiedReminderIds.add(entry.id)) {
        unawaited(
          widget.systemNotifier.show(
            PlannedNotification(
              id: entry.id,
              title: entry.title,
              body: entry.detail,
              at: entry.dueAt,
            ),
          ),
        );
      }
    }
    reminderDialogOpen = true;
    try {
      await showReminderDialog(
        context,
        entries: entries,
        onDismiss: dismissReminder,
        onSnooze: (entry, delay) {
          snoozedReminders[entry.id] = DateTime.now().add(delay);
          notifiedReminderIds.remove(entry.id);
          planSystemNotifications();
        },
        onOpen: openReminder,
      );
    } finally {
      reminderDialogOpen = false;
    }
  }

  Future<void> dismissReminder(ReminderEntry entry) async {
    switch (entry.kind) {
      case ReminderKind.event:
        final index = calendarItems.indexWhere(
          (item) => item.id == entry.itemId,
        );
        if (index < 0) return;
        final through = entry.occurrenceStart ?? calendarItems[index].startsAt;
        final current = calendarItems[index].reminderDismissedThrough;
        final updated = calendarItems[index].copyWith(
          reminderDismissedThrough: current != null && current.isAfter(through)
              ? current
              : through,
        );
        try {
          await widget.mailDataSource.saveCalendarEvent(updated);
        } on Object catch (error) {
          showPersistenceError(error);
          return;
        }
        if (mounted) setState(() => calendarItems[index] = updated);
      case ReminderKind.task:
        final index = tasks.indexWhere((item) => item.id == entry.itemId);
        if (index < 0) return;
        final updated = tasks[index].copyWith(reminderDismissed: true);
        try {
          await widget.mailDataSource.saveTask(updated);
        } on Object catch (error) {
          showPersistenceError(error);
          return;
        }
        if (mounted) setState(() => tasks[index] = updated);
    }
    snoozedReminders.remove(entry.id);
    planSystemNotifications();
  }

  void openReminder(ReminderEntry entry) {
    switch (entry.kind) {
      case ReminderKind.event:
        final event = calendarItems.cast<LocalCalendarItem?>().firstWhere(
          (item) => item!.id == entry.itemId,
          orElse: () => null,
        );
        if (event == null) return;
        setState(() {
          module = WorkspaceModule.calendar;
          calendarFocus = entry.occurrenceStart ?? event.startsAt;
        });
        unawaited(openEvent(event, occurrenceDate: entry.occurrenceStart));
      case ReminderKind.task:
        final task = tasks.cast<LocalTaskItem?>().firstWhere(
          (item) => item!.id == entry.itemId,
          orElse: () => null,
        );
        if (task == null) return;
        setState(() {
          module = WorkspaceModule.tasks;
          selectedTaskId = task.id;
        });
        unawaited(openTask(task));
    }
  }

  Future<void> showAccountSettings() async {
    final action = await showDialog<AccountManagementAction>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) => AccountManagerDialog(
        accounts: mailAccounts,
        davAccounts: davAccounts,
        collections: collections,
      ),
    );
    if (action == null || !mounted) return;
    if (action.type == AccountManagementActionType.aliases) {
      await editSenderAliases(action.account!);
      return;
    }
    if (action.type == AccountManagementActionType.addDav ||
        action.type == AccountManagementActionType.editDav) {
      await setupDavAccount(action.davAccount);
      return;
    }
    if (action.type == AccountManagementActionType.deleteDav) {
      await removeDavAccount(action.davAccount!);
      return;
    }
    if (action.type == AccountManagementActionType.delete) {
      final account = action.account!;
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (dialogContext) => AlertDialog(
          title: const Text('Konto entfernen?'),
          content: Text(
            '„${account.displayName}“ und alle lokal zwischengespeicherten '
            'Nachrichten dieses Kontos werden entfernt. Auf dem Mailserver '
            'bleiben die Nachrichten unverändert.',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialogContext, false),
              child: const Text('Abbrechen'),
            ),
            FilledButton(
              key: const Key('account-delete-confirm'),
              onPressed: () => Navigator.pop(dialogContext, true),
              child: const Text('Konto entfernen'),
            ),
          ],
        ),
      );
      if (confirmed != true || !mounted) return;
      await removeAccount(account);
      return;
    }

    final result = await showDialog<AccountSetupResult>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) => AccountSetupDialog(
        existing: action.type == AccountManagementActionType.edit
            ? action.account
            : null,
        onTest: widget.mailDataSource.testAccount,
        onTestOAuth: widget.mailDataSource.testOAuthAccount,
      ),
    );
    if (result == null) return;
    try {
      if (result.oauthTokens == null) {
        await widget.mailDataSource.saveAccount(
          result.account,
          result.password,
        );
      } else {
        await widget.mailDataSource.saveOAuthAccount(
          result.account,
          result.oauthTokens!,
        );
      }
    } on Object catch (error) {
      if (!mounted) return;
      showInformation(
        'Konto nicht gespeichert',
        'Konfiguration oder verschlüsselter Profilspeicher ist fehlgeschlagen.\n\n$error',
      );
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = mailAccounts.indexWhere(
        (account) => account.id == result.account.id,
      );
      if (index < 0) {
        mailAccounts.add(result.account);
      } else {
        mailAccounts[index] = result.account;
      }
      offlineMode = false;
      offlineModeChosen = false;
    });
    showNotice('Konto gespeichert. Die erste Synchronisierung startet.');
    await synchronize();
  }

  Future<void> setupDavAccount(DavAccountConfig? existing) async {
    final result = await showDavAccountDialog(
      context,
      existing: existing,
      onTest: widget.mailDataSource.testDavAccount,
    );
    if (result == null || !mounted) return;
    try {
      await widget.mailDataSource.saveDavAccount(
        result.account,
        result.password,
      );
    } on Object catch (error) {
      if (!mounted) return;
      showInformation(
        'Konto nicht gespeichert',
        'Der Server konnte nicht erreicht werden oder das verschlüsselte '
            'Profil hat die Änderung nicht übernommen.\n\n$error',
      );
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = davAccounts.indexWhere(
        (account) => account.id == result.account.id,
      );
      if (index < 0) {
        davAccounts.add(result.account);
      } else {
        davAccounts[index] = result.account;
      }
      offlineMode = false;
      offlineModeChosen = false;
    });
    showNotice('Konto gespeichert. Kalender und Kontakte werden abgeglichen.');
    await synchronize();
  }

  Future<void> removeDavAccount(DavAccountConfig account) async {
    final confirmed = await confirmDeletion(
      'Konto entfernen?',
      '„${account.displayName}“ wird mit allen lokal gespeicherten Kalendern, '
          'Aufgabenlisten und Adressbüchern dieses Kontos entfernt. Auf dem '
          'Server bleibt alles unverändert.',
    );
    if (!confirmed || !mounted) return;
    try {
      await widget.mailDataSource.deleteDavAccount(account.id);
    } on Object catch (error) {
      if (!mounted) return;
      showPersistenceError(error);
      return;
    }
    if (!mounted) return;
    setState(() {
      davAccounts.removeWhere((item) => item.id == account.id);
      final removed = collections
          .where((collection) => collection.accountId == account.id)
          .map((collection) => collection.id)
          .toSet();
      collections.removeWhere((collection) => removed.contains(collection.id));
      calendarItems.removeWhere(
        (event) => removed.contains(event.collectionId),
      );
      tasks.removeWhere((task) => removed.contains(task.collectionId));
      contacts.removeWhere((contact) => removed.contains(contact.collectionId));
      if (mailAccounts.isEmpty && davAccounts.isEmpty) offlineMode = true;
    });
    planSystemNotifications();
    showNotice('Das Konto wurde entfernt.');
  }

  Future<void> editSenderAliases(MailAccountConfig account) async {
    final aliases = await showDialog<List<MailSenderAlias>>(
      context: context,
      barrierDismissible: false,
      builder: (_) => SenderAliasesDialog(account: account),
    );
    if (aliases == null || !mounted) return;
    final List<MailSenderAlias> stored;
    try {
      stored = await widget.mailDataSource.saveAccountAliases(
        account.id,
        aliases,
      );
    } on Object catch (error) {
      if (!mounted) return;
      showInformation(
        'Absenderadressen nicht gespeichert',
        'Das verschlüsselte Profil konnte nicht aktualisiert werden.\n\n$error',
      );
      return;
    }
    if (!mounted) return;
    setState(() {
      final index = mailAccounts.indexWhere((item) => item.id == account.id);
      if (index >= 0) {
        mailAccounts[index] = mailAccounts[index].copyWith(aliases: stored);
      }
    });
    showNotice(
      stored.isEmpty
          ? 'Keine Aliase mehr für ${account.displayName}.'
          : 'Absenderadressen gespeichert. Wähle sie beim Schreiben im Feld „Von“.',
    );
  }

  /// Removes [account] from the profile and shows what is happening meanwhile.
  ///
  /// A running synchronization pass writes to the same profile, so the removal
  /// waits until that pass has finished and stops further passes. The progress
  /// dialog explains the wait and lets the user keep the account instead.
  Future<void> removeAccount(MailAccountConfig account) async {
    final navigator = Navigator.of(context, rootNavigator: true);
    final waitingForSync = ValueNotifier<bool>(synchronizing);
    var cancelled = false;
    final progress = DialogRoute<void>(
      context: context,
      barrierDismissible: false,
      themes: InheritedTheme.capture(from: context, to: navigator.context),
      builder: (_) => AccountRemovalDialog(
        accountName: account.displayName,
        waitingForSync: waitingForSync,
        onKeepAccount: () => cancelled = true,
      ),
    );
    void closeProgress() {
      if (!progress.isActive) return;
      if (progress.isCurrent) {
        navigator.pop();
      } else {
        navigator.removeRoute(progress);
      }
    }

    removingAccount = true;
    stopSynchronizationRequested = synchronizing;
    unawaited(navigator.push(progress));
    try {
      while (synchronizing && !cancelled && mounted) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      if (!mounted) return;
      if (cancelled) {
        closeProgress();
        showNotice('„${account.displayName}“ bleibt eingerichtet.');
        return;
      }
      waitingForSync.value = false;
      final WorkspaceDataSnapshot snapshot;
      try {
        snapshot = await widget.mailDataSource.deleteAccount(account.id);
      } on Object catch (error) {
        if (!mounted) return;
        closeProgress();
        showInformation(
          'Konto nicht entfernt',
          'Das verschlüsselte Profil hat die Änderung nicht übernommen.\n\n$error',
        );
        return;
      }
      if (!mounted) return;
      closeProgress();
      replaceWorkspace(snapshot);
      showNotice(
        'Das Konto und seine Zugangsdaten wurden aus dem Profil entfernt.',
      );
    } finally {
      removingAccount = false;
      stopSynchronizationRequested = false;
      if (mounted) closeProgress();
    }
  }

  void replaceWorkspace(WorkspaceDataSnapshot snapshot) {
    searchDebounce?.cancel();
    searchGeneration += 1;
    final previouslySelectedId = selectedMail?.id;
    final previouslySelectedFolder = selectedFolder;
    final activeQuery = query;
    final includeContent = searchIncludesContent;
    final shouldAdoptTheme =
        snapshot.darkModeEnabled != widget.darkModeEnabled &&
        widget.onDarkModeChanged != null;
    setState(() {
      folders = snapshot.folders.toList();
      favoriteFolderIds = snapshot.favoriteFolderIds
          .where((id) => snapshot.folders.any((folder) => folder.id == id))
          .toList();
      collapsedFolderIds = snapshot.collapsedFolderIds.toSet();
      folderOrderIds = snapshot.folderOrderIds
          .where((id) => snapshot.folders.any((folder) => folder.id == id))
          .toList();
      messages = snapshot.messages.toList();
      calendarItems = snapshot.calendarEvents.toList();
      tasks = snapshot.tasks.toList();
      contacts = snapshot.contacts.toList();
      mailAccounts = snapshot.mailAccounts.toList();
      davAccounts = snapshot.davAccounts.toList();
      collections = snapshot.collections.toList();
      pendingMailOperations = snapshot.pendingMailOperations;
      catalogMessagesRemaining = snapshot.catalogMessagesRemaining;
      deltaMailboxesSynchronized = snapshot.deltaMailboxesSynchronized;
      fullMailboxesReconciled = snapshot.fullMailboxesReconciled;
      qresyncMailboxesSynchronized = snapshot.qresyncMailboxesSynchronized;
      profileSearchResults = null;
      searchInProgress = false;
      searchIncludesContent = false;
      if (!folders.any((folder) => folder.id == selectedFolder)) {
        selectedFolder = folderIdForRole('inbox');
      }
      final refreshedVisible = filteredMessages;
      final refreshedIndex = previouslySelectedId == null
          ? -1
          : refreshedVisible.indexWhere(
              (message) => message.id == previouslySelectedId,
            );
      selectedMessage = refreshedIndex >= 0
          ? refreshedIndex
          : refreshedVisible.isEmpty
          ? 0
          : selectedMessage.clamp(0, refreshedVisible.length - 1);
    });
    if (shouldAdoptTheme) {
      unawaited(adoptSnapshotTheme(snapshot.darkModeEnabled));
    }
    if (selectedFolder != previouslySelectedFolder) restartIdleWatcher();
    if (activeQuery.trim().isNotEmpty) {
      searchWorkspace(activeQuery, includeContent: includeContent);
    }
  }

  Future<void> adoptSnapshotTheme(bool enabled) async {
    try {
      await widget.onDarkModeChanged?.call(enabled);
    } on Object catch (error) {
      if (mounted) showPersistenceError(error);
    }
  }

  /// Looks for a newer release and offers it. Automatic checks run at most
  /// daily, never in offline mode, and fail silently; a check the user
  /// started reports every outcome.
  Future<void> checkForUpdates({required bool automatic}) async {
    final service = widget.appUpdates;
    if (service == null || !service.supported || updateCheckRunning) return;
    final userOffline = offlineMode && offlineModeChosen;
    if (automatic && (userOffline || !service.automaticCheckDue)) return;
    if (!automatic && userOffline) {
      showNotice('Im Offline-Modus wird nicht nach Updates gesucht.');
      return;
    }
    updateCheckRunning = true;
    try {
      final update = await service.check(automatic: automatic);
      if (!mounted) return;
      if (update == null) {
        if (!automatic) showNotice('MAICENTA ist auf dem neuesten Stand.');
        return;
      }
      await showAppUpdateDialog(context, service: service, update: update);
    } on Object catch (error) {
      if (!automatic && mounted) {
        showInformation(
          'Update-Suche fehlgeschlagen',
          'Die Release-Liste konnte nicht abgerufen werden.\n\n$error',
        );
      }
    } finally {
      updateCheckRunning = false;
    }
  }

  void showNotice(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(message)));
  }

  void showPersistenceError(Object error) {
    if (!mounted) return;
    showInformation(
      'Änderung nicht gespeichert',
      'Die lokale Datenbank konnte die Änderung nicht übernehmen.\n\n$error',
    );
  }

  void showInformation(String title, String message) {
    showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(title),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Schließen'),
          ),
        ],
      ),
    );
  }

  Future<void> saveAttachment(MailAttachmentData attachment) async {
    try {
      final destination = await getSaveLocation(
        suggestedName: attachment.fileName,
        confirmButtonText: 'Speichern',
      );
      if (destination == null) return;
      await widget.mailDataSource.exportAttachment(
        attachment.id,
        destination.path,
      );
      if (!mounted) return;
      showNotice('„${attachment.fileName}“ wurde gespeichert.');
    } on Object catch (error) {
      if (!mounted) return;
      showInformation(
        'Anhang konnte nicht gespeichert werden',
        error.toString(),
      );
    }
  }

  Future<void> exportWorkspace() async {
    final action = await showDialog<ProfileTransferAction>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Profil sichern oder wiederherstellen'),
        content: const SizedBox(
          width: 440,
          child: Text(
            'Eine vollständige Profilsicherung enthält Konten, Nachrichten, '
            'Anhänge, Kalender, Aufgaben, Kontakte und Zugangsdaten. Sie ist '
            'immer mit einem eigenen Exportpasswort verschlüsselt.',
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext),
            child: const Text('Abbrechen'),
          ),
          OutlinedButton.icon(
            onPressed: () =>
                Navigator.pop(dialogContext, ProfileTransferAction.import),
            icon: const Icon(Icons.file_download_outlined),
            label: const Text('Importieren'),
          ),
          FilledButton.icon(
            onPressed: () =>
                Navigator.pop(dialogContext, ProfileTransferAction.export),
            icon: const Icon(Icons.lock_outline),
            label: const Text('Sicherung erstellen'),
          ),
        ],
      ),
    );
    if (!mounted || action == null) return;
    if (action == ProfileTransferAction.export) {
      await createProfileBackup();
    } else {
      await restoreProfileBackup();
    }
  }

  Future<void> createProfileBackup() async {
    final timestamp = DateTime.now().toUtc().toIso8601String().replaceAll(
      ':',
      '-',
    );
    final destination = await getSaveLocation(
      suggestedName: 'maicenta-profile-$timestamp.maicenta-profile',
      confirmButtonText: 'Sicherung erstellen',
      acceptedTypeGroups: const [
        XTypeGroup(label: 'MAICENTA-Profil', extensions: ['maicenta-profile']),
      ],
    );
    if (!mounted || destination == null) return;
    final password = await requestProfilePassword(confirmPassword: true);
    if (!mounted || password == null) return;
    try {
      await widget.mailDataSource.exportProfile(destination.path, password);
      if (!mounted) return;
      showInformation(
        'Profilsicherung erstellt',
        'Das vollständige verschlüsselte Profil wurde gespeichert unter:\n\n'
            '${destination.path}\n\nBewahre das Exportpasswort getrennt auf. '
            'Ohne dieses Passwort kann die Sicherung nicht wiederhergestellt werden.',
      );
    } on Object catch (error) {
      if (!mounted) return;
      showInformation('Export fehlgeschlagen', error.toString());
    }
  }

  Future<void> restoreProfileBackup() async {
    final source = await openFile(
      acceptedTypeGroups: const [
        XTypeGroup(label: 'MAICENTA-Profil', extensions: ['maicenta-profile']),
      ],
    );
    if (!mounted || source == null) return;
    final password = await requestProfilePassword(confirmPassword: false);
    if (!mounted || password == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Aktuelles Profil ersetzen?'),
        content: const Text(
          'Die ausgewählte Sicherung ersetzt das aktuell geöffnete lokale '
          'Profil. Während des Imports bleibt eine Rückrollkopie erhalten.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Profil wiederherstellen'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    try {
      final snapshot = await widget.mailDataSource.importProfile(
        source.path,
        password,
      );
      if (!mounted) return;
      replaceWorkspace(snapshot);
      setState(() {
        offlineMode = mailAccounts.isEmpty;
        offlineModeChosen = false;
      });
      showInformation(
        'Profil wiederhergestellt',
        'Konten, lokale Daten und enthaltene Zugangsdaten wurden aus der '
            'verschlüsselten Sicherung übernommen.',
      );
    } on Object catch (error) {
      if (!mounted) return;
      showInformation('Import fehlgeschlagen', error.toString());
    }
  }

  Future<String?> requestProfilePassword({
    required bool confirmPassword,
  }) async {
    final password = TextEditingController();
    final confirmation = TextEditingController();
    String? validationError;
    final result = await showDialog<String>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (context, updateDialog) => AlertDialog(
          title: Text(
            confirmPassword ? 'Exportpasswort festlegen' : 'Exportpasswort',
          ),
          content: SizedBox(
            width: 420,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextField(
                  controller: password,
                  obscureText: true,
                  autofocus: true,
                  decoration: InputDecoration(
                    labelText: 'Passwort',
                    helperText: confirmPassword
                        ? 'Mindestens 12 Zeichen; nicht wiederherstellbar'
                        : null,
                    errorText: validationError,
                  ),
                ),
                if (confirmPassword) ...[
                  const SizedBox(height: 12),
                  TextField(
                    controller: confirmation,
                    obscureText: true,
                    decoration: const InputDecoration(
                      labelText: 'Passwort wiederholen',
                    ),
                    onSubmitted: (_) {},
                  ),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(dialogContext),
              child: const Text('Abbrechen'),
            ),
            FilledButton(
              onPressed: () {
                final value = password.text;
                if (value.characters.length < 12) {
                  updateDialog(
                    () =>
                        validationError = 'Mindestens 12 Zeichen erforderlich',
                  );
                  return;
                }
                if (confirmPassword && value != confirmation.text) {
                  updateDialog(
                    () => validationError =
                        'Die Passwörter stimmen nicht überein',
                  );
                  return;
                }
                Navigator.pop(dialogContext, value);
              },
              child: Text(confirmPassword ? 'Verschlüsseln' : 'Entsperren'),
            ),
          ],
        ),
      ),
    );
    password.dispose();
    confirmation.dispose();
    return result;
  }

  void showOptions() {
    showDialog<void>(
      context: context,
      builder: (dialogContext) => StatefulBuilder(
        builder: (context, updateDialog) => AlertDialog(
          title: const Text('Optionen'),
          content: SizedBox(
            width: 420,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  SwitchListTile(
                    key: const Key('dark-mode-toggle'),
                    secondary: Icon(
                      widget.darkModeEnabled
                          ? Icons.dark_mode_outlined
                          : Icons.light_mode_outlined,
                    ),
                    title: const Text('Dunkler Modus'),
                    subtitle: const Text(
                      'Darstellung für dieses Profil verwenden',
                    ),
                    value: widget.darkModeEnabled,
                    onChanged: (value) async {
                      final changeMode = widget.onDarkModeChanged;
                      if (changeMode == null) return;
                      try {
                        await changeMode(value);
                        if (dialogContext.mounted) updateDialog(() {});
                      } on Object catch (error) {
                        if (!mounted) return;
                        showPersistenceError(error);
                      }
                    },
                  ),
                  SwitchListTile(
                    title: const Text('Offline-Modus'),
                    subtitle: const Text('Keine Netzwerkverbindungen aufbauen'),
                    value: offlineMode,
                    onChanged: (value) {
                      setState(() {
                        offlineMode = value;
                        offlineModeChosen = value;
                      });
                      updateDialog(() {});
                      restartIdleWatcher();
                    },
                  ),
                  SwitchListTile(
                    title: const Text('Ordnerbereich anzeigen'),
                    value: showFolderPane,
                    onChanged: (value) {
                      setState(() => showFolderPane = value);
                      updateDialog(() {});
                    },
                  ),
                  SwitchListTile(
                    title: const Text('Lesebereich anzeigen'),
                    value: showReadingPane,
                    onChanged: (value) {
                      setState(() => showReadingPane = value);
                      updateDialog(() {});
                    },
                  ),
                  const ListTile(
                    leading: Icon(Icons.shield_outlined),
                    title: Text('HTML-Sicherheitsfilter'),
                    subtitle: Text('Immer aktiv'),
                  ),
                  if (widget.appUpdates case final updates?) ...[
                    SwitchListTile(
                      key: const Key('automatic-update-check-toggle'),
                      secondary: const Icon(Icons.update),
                      title: const Text('Automatisch nach Updates suchen'),
                      subtitle: Text(
                        updates.supported
                            ? 'Einmal täglich die Releases auf GitHub prüfen'
                            : 'Nur in mit dem Installer eingerichteten '
                                  'Versionen verfügbar',
                      ),
                      value:
                          updates.supported &&
                          updates.preferences.automaticChecks,
                      onChanged: updates.supported
                          ? (value) async {
                              await updates.setAutomaticChecks(value);
                              if (dialogContext.mounted) updateDialog(() {});
                            }
                          : null,
                    ),
                    ListTile(
                      key: const Key('options-check-updates'),
                      leading: const Icon(Icons.system_update_alt_outlined),
                      title: const Text('Nach Updates suchen'),
                      enabled: updates.supported,
                      onTap: () {
                        Navigator.pop(dialogContext);
                        unawaited(checkForUpdates(automatic: false));
                      },
                    ),
                  ],
                  ListTile(
                    key: const Key('options-about'),
                    leading: const Icon(Icons.info_outline),
                    title: const Text('Über MAICENTA'),
                    subtitle: const Text(
                      'Version, Plattform, Lizenzen und Fehlermeldung',
                    ),
                    trailing: const Icon(Icons.chevron_right, size: 18),
                    onTap: () => showMaicentaAboutDialog(
                      dialogContext,
                      loadBuildInfo: widget.loadBuildInfo,
                    ),
                  ),
                ],
              ),
            ),
          ),
          actions: [
            FilledButton(
              onPressed: () => Navigator.pop(dialogContext),
              child: const Text('Fertig'),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Column(
        children: [
          AppTitleBar(
            moduleLabel: switch (module) {
              WorkspaceModule.mail => 'E-Mail',
              WorkspaceModule.calendar => 'Kalender',
              WorkspaceModule.tasks => 'Aufgaben',
              WorkspaceModule.contacts => 'Kontakte',
            },
            onSearch: searchWorkspace,
            searching: searchInProgress,
            searchActive: query.trim().isNotEmpty,
            includesContent: searchIncludesContent,
            onIncludeContent: includeMessageContentInSearch,
            onSynchronize: synchronize,
            onNotifications: () => showInformation(
              'Benachrichtigungen',
              'Keine neuen Benachrichtigungen. Der lokale Workspace ist bereit.',
            ),
            onSettings: () => showInformation(
              'Einstellungen',
              'Datenschutz: Externe Mail-Inhalte werden blockiert. Der Offline-Modus kann im Reiter „Senden/Empfangen“ geändert werden.',
            ),
            onAppMenu: () => showInformation(
              'MAICENTA-Module',
              'E-Mail, Kalender, Aufgaben und Kontakte sind unten im klassischen Navigationsbereich verfügbar.',
            ),
            onProfile: () => showInformation(
              'Lokales Profil',
              mailAccounts.isEmpty
                  ? 'Noch kein Mailkonto eingerichtet.\n\nUnter Datei → Kontoeinstellungen fügst du ein Konto hinzu.'
                  : mailAccounts
                        .map(
                          (account) =>
                              '${account.displayName}\n${account.email}',
                        )
                        .join('\n\n'),
            ),
          ),
          Ribbon(
            module: module,
            offlineMode: offlineMode,
            selectedIsDraft: selectedMail?.draft ?? false,
            commands: RibbonCommands(
              newItem: handleNewItem,
              editDraft: editSelectedDraft,
              reply: replyToSelected,
              forward: forwardSelected,
              archive: () => moveSelectedTo('archive', 'archiviert'),
              delete: () =>
                  moveSelectedTo('trash', 'in den Papierkorb verschoben'),
              toggleFlag: toggleSelectedFlag,
              synchronize: synchronize,
              toggleOffline: toggleOffline,
              showProgress: () => showInformation(
                'Synchronisierungsstatus',
                [
                  pendingMailOperations == 0
                      ? 'Keine ausstehenden Serveränderungen.'
                      : '$pendingMailOperations Serveränderungen warten auf den nächsten IMAP-Abgleich.',
                  if (synchronizing) 'Der Abgleich mit den Konten läuft.',
                  if (catalogMessagesRemaining > 0)
                    '$catalogMessagesRemaining Nachrichtenmetadaten werden noch katalogisiert.',
                  if (deltaMailboxesSynchronized > 0 ||
                      fullMailboxesReconciled > 0)
                    'Letzter Abgleich: $deltaMailboxesSynchronized Ordner per Delta, '
                        '$fullMailboxesReconciled als Vollabgleich.',
                  if (qresyncMailboxesSynchronized > 0)
                    '$qresyncMailboxesSynchronized Ordner nutzten QRESYNC für Löschungsänderungen.',
                ].join('\n'),
              ),
              newFolder: createFolder,
              renameFolder: renameFolder,
              deleteFolder: deleteFolder,
              markAllRead: markAllRead,
              toggleFolderPane: () =>
                  setState(() => showFolderPane = !showFolderPane),
              toggleReadingPane: () =>
                  setState(() => showReadingPane = !showReadingPane),
              cycleSort: cycleSort,
              cycleZoom: cycleZoom,
              accountSettings: showAccountSettings,
              importExport: exportWorkspace,
              options: showOptions,
            ),
          ),
          Expanded(child: moduleView()),
          StatusBar(
            module: module,
            summary: moduleSummary(),
            itemCount: messages.length,
            unreadCount: messages
                .where((message) => message.unread && !message.draft)
                .length,
            pendingMailOperations: pendingMailOperations,
            synchronizing: synchronizing,
            catalogMessagesRemaining: catalogMessagesRemaining,
            offlineMode: offlineMode,
            zoom: readingZoom,
          ),
        ],
      ),
    );
  }

  Widget moduleView() {
    return switch (module) {
      WorkspaceModule.mail => MailWorkspace(
        messages: filteredMessages,
        folders: visibleFolders,
        accounts: mailAccounts,
        favoriteFolderIds: favoriteFolderIds,
        collapsedFolderIds: collapsedFolderIds,
        onToggleFolderCollapsed: (folderId) =>
            unawaited(toggleFolderCollapsed(folderId)),
        folderOrderIds: folderOrderIds,
        onFolderOrderChanged: (folderIds) =>
            unawaited(updateFolderOrder(folderIds)),
        selectedMessage: selectedMessage,
        selectedFolder: selectedFolder,
        onMessageSelected: selectMessage,
        onMessageContextSelected: selectMessageForContext,
        onMessageOpened: openMessageWindow,
        onMessageContextAction: handleMailContextAction,
        onFolderSelected: selectFolder,
        onMessageDropped: (message, folder) =>
            unawaited(moveMessageByDrop(message, folder)),
        onFavoriteFolderOrderChanged: (folderIds) =>
            unawaited(updateFavoriteFolders(folderIds)),
        totalMessageCount:
            query.trim().isNotEmpty || selectedFolder.startsWith('virtual.')
            ? filteredMessages.length
            : selectedFolderData?.totalCount ?? filteredMessages.length,
        hasMoreMessages: canLoadMoreMessages,
        loadingMoreMessages: loadingMoreMessages,
        onLoadMoreMessages: loadMoreMessages,
        onReply: replyToSelected,
        onForward: forwardSelected,
        onEditDraft: editSelectedDraft,
        onSaveAttachment: saveAttachment,
        onReloadContent: reloadMessageContent,
        onAccountSettings: showAccountSettings,
        onNewFolder: createFolder,
        filter: mailFilter,
        onFilterChanged: (value) => setState(() {
          mailFilter = value;
          selectedMessage = 0;
        }),
        showFolderPane: showFolderPane,
        showReadingPane: showReadingPane,
        readingZoom: readingZoom,
        module: module,
        onModuleSelected: selectModule,
        onMoreApps: showMoreApps,
      ),
      WorkspaceModule.calendar => ClassicModuleWorkspace(
        module: module,
        onModuleSelected: selectModule,
        onMoreApps: showMoreApps,
        sidebar: Padding(
          padding: const EdgeInsets.fromLTRB(12, 10, 12, 0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              MiniCalendar(
                focused: calendarFocus,
                events: visibleCalendarEvents,
                onSelected: (day) => setState(() {
                  calendarFocus = day;
                  if (calendarView == CalendarView.month &&
                      day.month != calendarFocus.month) {
                    calendarView = CalendarView.day;
                  }
                }),
              ),
              const Divider(height: 28),
              Text(
                'MEINE KALENDER',
                style: TextStyle(
                  fontSize: 10,
                  fontWeight: FontWeight.w700,
                  color: MaicentaPalette.of(context).mutedText,
                ),
              ),
              CollectionToggle(
                key: const Key('calendar-personal-toggle'),
                label: 'Persönlicher Kalender',
                color: MaicentaApp.primaryBlue,
                enabled: calendarEnabled,
                onChanged: (value) => setState(() => calendarEnabled = value),
              ),
              for (final calendar in eventCalendars)
                CollectionToggle(
                  key: Key('collection-toggle-${calendar.id}'),
                  label: calendar.name,
                  color:
                      parseCssColor(calendar.color) ?? MaicentaApp.primaryBlue,
                  enabled: calendar.enabled,
                  onChanged: (value) =>
                      unawaited(toggleCollection(calendar, value)),
                ),
              if (davAccounts.isEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: TextButton.icon(
                    key: const Key('calendar-add-account'),
                    onPressed: () => unawaited(setupDavAccount(null)),
                    icon: const Icon(Icons.cloud_sync_outlined, size: 16),
                    label: const Text(
                      'Kalenderkonto verbinden',
                      style: TextStyle(fontSize: 12),
                    ),
                  ),
                ),
            ],
          ),
        ),
        child: CalendarWorkspace(
          events: visibleCalendarEvents,
          focused: calendarFocus,
          view: calendarView,
          colorOf: colorForEvent,
          onFocusedChanged: (day) => setState(() => calendarFocus = day),
          onViewChanged: (view) => setState(() => calendarView = view),
          onOpenEvent: (occurrence) => unawaited(
            openEvent(occurrence.event, occurrenceDate: occurrence.date),
          ),
          onCreateAt: (start, allDay) => unawaited(
            openEvent(null, initialStart: start, initialAllDay: allDay),
          ),
        ),
      ),
      WorkspaceModule.tasks => ClassicModuleWorkspace(
        module: module,
        onModuleSelected: selectModule,
        onMoreApps: showMoreApps,
        sidebar: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TaskFilterList(
              tasks: visibleTasks,
              selected: taskFilter,
              onSelected: (filter) => setState(() => taskFilter = filter),
            ),
            if (taskLists.isNotEmpty) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(13, 16, 10, 2),
                child: Text(
                  'AUFGABENLISTEN',
                  style: TextStyle(
                    fontSize: 10,
                    fontWeight: FontWeight.w700,
                    color: MaicentaPalette.of(context).mutedText,
                  ),
                ),
              ),
              for (final list in taskLists)
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: CollectionToggle(
                    key: Key('collection-toggle-${list.id}'),
                    label: list.name,
                    color: parseCssColor(list.color) ?? MaicentaApp.primaryBlue,
                    enabled: list.enabled,
                    onChanged: (value) =>
                        unawaited(toggleCollection(list, value)),
                  ),
                ),
            ],
          ],
        ),
        child: TasksWorkspace(
          tasks: visibleTasks,
          filter: taskFilter,
          selectedId: selectedTaskId,
          onSelected: (task) => setState(() => selectedTaskId = task.id),
          onToggle: (task) => unawaited(toggleTask(task)),
          onOpen: (task) => unawaited(openTask(task)),
          onDelete: (task) => unawaited(confirmDeleteTask(task)),
          onQuickAdd: (title) => unawaited(quickAddTask(title)),
        ),
      ),
      WorkspaceModule.contacts => ClassicModuleWorkspace(
        module: module,
        onModuleSelected: selectModule,
        onMoreApps: showMoreApps,
        sidebar: addressBooks.isEmpty
            ? null
            : Padding(
                padding: const EdgeInsets.fromLTRB(12, 10, 12, 0),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      'ADRESSBÜCHER',
                      style: TextStyle(
                        fontSize: 10,
                        fontWeight: FontWeight.w700,
                        color: MaicentaPalette.of(context).mutedText,
                      ),
                    ),
                    CollectionToggle(
                      label: 'Meine Kontakte',
                      color: MaicentaApp.primaryBlue,
                      enabled: true,
                      onChanged: null,
                    ),
                    for (final book in addressBooks)
                      CollectionToggle(
                        key: Key('collection-toggle-${book.id}'),
                        label: book.name,
                        color:
                            parseCssColor(book.color) ??
                            MaicentaApp.primaryBlue,
                        enabled: book.enabled,
                        onChanged: (value) =>
                            unawaited(toggleCollection(book, value)),
                      ),
                  ],
                ),
              ),
        child: ContactsWorkspace(
          contacts: visibleContacts,
          query: contactQuery,
          onQueryChanged: (value) => setState(() => contactQuery = value),
          selectedId: selectedContactId,
          onSelected: (contact) =>
              setState(() => selectedContactId = contact.id),
          onOpen: (contact) => unawaited(openContact(contact)),
          onDelete: (contact) => unawaited(confirmDeleteContact(contact)),
          onCompose: (contact, address) =>
              unawaited(composeToContact(contact, address)),
        ),
      ),
    };
  }

  /// Status-bar text for the calendar, task, and contact modules.
  String moduleSummary() {
    final today = dateOnly(DateTime.now());
    switch (module) {
      case WorkspaceModule.mail:
        return '';
      case WorkspaceModule.calendar:
        final events = visibleCalendarEvents;
        final todayCount = expandEvents(
          events,
          today,
          today.add(const Duration(days: 1)),
        ).length;
        return '${events.length} Termine · $todayCount heute';
      case WorkspaceModule.tasks:
        final open = tasks.where((task) => !task.done).length;
        final overdue = tasks.where((task) => task.overdue).length;
        return '$open offene Aufgaben'
            '${overdue > 0 ? ' · $overdue überfällig' : ''}';
      case WorkspaceModule.contacts:
        return contacts.length == 1
            ? '1 Kontakt'
            : '${contacts.length} Kontakte';
    }
  }

  void showMoreApps() {
    showInformation(
      'Weitere Apps',
      'Notizen, Vault, Assistant und Erweiterungen folgen in späteren Roadmap-Phasen.',
    );
  }
}

class AccountSetupResult {
  const AccountSetupResult({
    required this.account,
    this.password = '',
    this.oauthTokens,
  });

  final MailAccountConfig account;
  final String password;
  final MailOAuthTokens? oauthTokens;
}

enum AccountManagementActionType {
  add,
  edit,
  aliases,
  delete,
  addDav,
  editDav,
  deleteDav,
}

class AccountManagementAction {
  const AccountManagementAction(this.type, [this.account, this.davAccount]);

  final AccountManagementActionType type;
  final MailAccountConfig? account;
  final DavAccountConfig? davAccount;
}

/// Checkbox row with a color swatch for a calendar, list, or address book.
class CollectionToggle extends StatelessWidget {
  const CollectionToggle({
    super.key,
    required this.label,
    required this.color,
    required this.enabled,
    required this.onChanged,
  });

  final String label;
  final Color color;
  final bool enabled;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: CheckboxListTile(
        value: enabled,
        onChanged: onChanged == null
            ? null
            : (value) => onChanged!(value ?? true),
        dense: true,
        contentPadding: EdgeInsets.zero,
        controlAffinity: ListTileControlAffinity.leading,
        activeColor: color,
        title: Row(
          children: [
            Container(
              width: 10,
              height: 10,
              margin: const EdgeInsets.only(right: 6),
              decoration: BoxDecoration(color: color, shape: BoxShape.circle),
            ),
            Expanded(
              child: Text(
                label,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 12),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class AccountManagerDialog extends StatelessWidget {
  const AccountManagerDialog({
    super.key,
    required this.accounts,
    this.davAccounts = const [],
    this.collections = const [],
  });

  final List<MailAccountConfig> accounts;
  final List<DavAccountConfig> davAccounts;
  final List<WorkspaceCollection> collections;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    Widget sectionTitle(String title) => Padding(
      padding: const EdgeInsets.fromLTRB(0, 12, 0, 4),
      child: Text(
        title,
        style: TextStyle(
          fontSize: 11,
          fontWeight: FontWeight.w700,
          color: palette.mutedText,
        ),
      ),
    );
    return AlertDialog(
      title: const Text('Kontoeinstellungen'),
      content: SizedBox(
        width: 680,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 460),
          child: ListView(
            shrinkWrap: true,
            children: [
              sectionTitle('E-MAIL-KONTEN'),
              if (accounts.isEmpty)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 12),
                  child: Text(
                    'Noch kein echtes Mailkonto eingerichtet. Füge ein '
                    'IMAP-/SMTP-Konto hinzu, um Nachrichten zu synchronisieren.',
                  ),
                ),
              for (final account in accounts) ...[
                ListTile(
                  key: Key('account-row-${account.id}'),
                  leading: const CircleAvatar(
                    child: Icon(Icons.alternate_email),
                  ),
                  title: Text(account.displayName),
                  subtitle: Text(
                    '${account.email}'
                    '${account.aliases.isEmpty ? '' : ' · ${account.aliases.length == 1 ? '1 Alias' : '${account.aliases.length} Aliase'}'}'
                    '\n${account.imapHost} · '
                    '${account.lastSyncAt == null ? 'Noch nicht synchronisiert' : 'Zuletzt synchronisiert: ${formatDateTime(account.lastSyncAt!.toLocal())}'}',
                  ),
                  isThreeLine: true,
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        key: Key('account-edit-${account.id}'),
                        tooltip: 'Bearbeiten',
                        onPressed: () => Navigator.pop(
                          context,
                          AccountManagementAction(
                            AccountManagementActionType.edit,
                            account,
                          ),
                        ),
                        icon: const Icon(Icons.edit_outlined),
                      ),
                      IconButton(
                        key: Key('account-aliases-${account.id}'),
                        tooltip: 'Absenderadressen',
                        onPressed: () => Navigator.pop(
                          context,
                          AccountManagementAction(
                            AccountManagementActionType.aliases,
                            account,
                          ),
                        ),
                        icon: const Icon(Icons.alternate_email),
                      ),
                      IconButton(
                        key: Key('account-delete-${account.id}'),
                        tooltip: 'Entfernen',
                        onPressed: () => Navigator.pop(
                          context,
                          AccountManagementAction(
                            AccountManagementActionType.delete,
                            account,
                          ),
                        ),
                        icon: const Icon(Icons.delete_outline),
                      ),
                    ],
                  ),
                ),
                const Divider(height: 1),
              ],
              sectionTitle('KALENDER- UND KONTAKTKONTEN (CALDAV/CARDDAV)'),
              if (davAccounts.isEmpty)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 12),
                  child: Text(
                    'Noch kein Kalender- oder Kontaktkonto verbunden. Ein '
                    'CalDAV/CardDAV-Konto gleicht Kalender, Aufgabenlisten und '
                    'Adressbücher in beide Richtungen ab.',
                  ),
                ),
              for (final account in davAccounts) ...[
                ListTile(
                  key: Key('dav-account-row-${account.id}'),
                  leading: const CircleAvatar(
                    child: Icon(Icons.cloud_sync_outlined),
                  ),
                  title: Text(account.displayName),
                  subtitle: Text(
                    '${account.username} · ${account.serverUrl}\n'
                    '${collections.where((collection) => collection.accountId == account.id).map((collection) => collection.name).join(', ')}'
                    '${account.lastSyncAt == null ? '' : ' · Zuletzt: ${formatDateTime(account.lastSyncAt!.toLocal())}'}',
                  ),
                  isThreeLine: true,
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        key: Key('dav-account-edit-${account.id}'),
                        tooltip: 'Bearbeiten',
                        onPressed: () => Navigator.pop(
                          context,
                          AccountManagementAction(
                            AccountManagementActionType.editDav,
                            null,
                            account,
                          ),
                        ),
                        icon: const Icon(Icons.edit_outlined),
                      ),
                      IconButton(
                        key: Key('dav-account-delete-${account.id}'),
                        tooltip: 'Entfernen',
                        onPressed: () => Navigator.pop(
                          context,
                          AccountManagementAction(
                            AccountManagementActionType.deleteDav,
                            null,
                            account,
                          ),
                        ),
                        icon: const Icon(Icons.delete_outline),
                      ),
                    ],
                  ),
                ),
                const Divider(height: 1),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Schließen'),
        ),
        OutlinedButton.icon(
          key: const Key('dav-account-add'),
          onPressed: () => Navigator.pop(
            context,
            const AccountManagementAction(AccountManagementActionType.addDav),
          ),
          icon: const Icon(Icons.cloud_sync_outlined),
          label: const Text('Kalender/Kontakte verbinden'),
        ),
        FilledButton.icon(
          key: const Key('account-add'),
          onPressed: () => Navigator.pop(
            context,
            const AccountManagementAction(AccountManagementActionType.add),
          ),
          icon: const Icon(Icons.add),
          label: const Text('E-Mail-Konto hinzufügen'),
        ),
      ],
    );
  }
}

class AccountSetupDialog extends StatefulWidget {
  const AccountSetupDialog({
    super.key,
    this.existing,
    required this.onTest,
    this.onTestOAuth,
    this.onDetect,
    this.onAuthorizeOAuth,
  });

  final MailAccountConfig? existing;
  final Future<void> Function(MailAccountConfig account, String password)
  onTest;
  final Future<void> Function(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  )?
  onTestOAuth;

  /// Probes the address for the best setup method. Defaults to the live
  /// detection; tests inject a fake.
  final MailSetupDetector? onDetect;
  final Future<MailOAuthTokens> Function(
    MailOAuthProvider provider,
    String email,
  )?
  onAuthorizeOAuth;

  @override
  State<AccountSetupDialog> createState() => _AccountSetupDialogState();
}

enum _AccountSetupStep { identity, method }

class _AccountSetupDialogState extends State<AccountSetupDialog> {
  late String accountId;
  final displayName = TextEditingController();
  final email = TextEditingController();
  final imapHost = TextEditingController();
  final imapPort = TextEditingController(text: '993');
  final imapUsername = TextEditingController();
  final smtpHost = TextEditingController();
  final smtpPort = TextEditingController(text: '587');
  final smtpUsername = TextEditingController();
  final password = TextEditingController();
  String imapSecurity = 'tls';
  String smtpSecurity = 'starttls';
  _AccountSetupStep step = _AccountSetupStep.identity;
  MailSetupDetection? detection;
  MailSetupSuggestion? selected;
  MailOAuthTokens? oauthTokens;
  String? status;
  bool statusSuccess = false;
  bool busy = false;
  bool showAdvanced = false;
  int? testedConfiguration;

  MailSetupMethod get method => selected?.method ?? MailSetupMethod.manual;

  bool get isEditing => widget.existing != null;

  @override
  void initState() {
    super.initState();
    loadAccount(widget.existing);
  }

  void loadAccount(MailAccountConfig? account) {
    accountId =
        account?.id ?? 'account.${DateTime.now().microsecondsSinceEpoch}';
    displayName.text = account?.displayName ?? '';
    email.text = account?.email ?? '';
    imapHost.text = account?.imapHost ?? '';
    imapPort.text = '${account?.imapPort ?? 993}';
    imapSecurity = account?.imapSecurity ?? 'tls';
    imapUsername.text = account?.imapUsername ?? '';
    smtpHost.text = account?.smtpHost ?? '';
    smtpPort.text = '${account?.smtpPort ?? 587}';
    smtpSecurity = account?.smtpSecurity ?? 'starttls';
    smtpUsername.text = account?.smtpUsername ?? '';
    oauthTokens = null;
    password.clear();
    status = null;
    statusSuccess = false;
    showAdvanced = false;
    testedConfiguration = null;
    if (account == null) {
      step = _AccountSetupStep.identity;
      detection = null;
      selected = null;
      return;
    }
    // Editing never re-detects: the stored method stays the recommendation,
    // every other supported method remains one click away.
    detection = MailSetupDetection.forExistingAccount(
      emailAddress: account.email,
      method: _methodOf(account),
      storedSettings: DiscoveredMailSettings(
        imapHost: account.imapHost,
        imapPort: account.imapPort,
        imapSecurity: account.imapSecurity,
        imapUsername: account.imapUsername,
        smtpHost: account.smtpHost,
        smtpPort: account.smtpPort,
        smtpSecurity: account.smtpSecurity,
        smtpUsername: account.smtpUsername,
        source: 'Gespeichert',
      ),
    );
    selected = detection!.recommended;
    step = _AccountSetupStep.method;
  }

  static MailSetupMethod _methodOf(MailAccountConfig account) {
    if (account.authentication != 'oauth2') return MailSetupMethod.manual;
    if (account.provider == 'microsoft_graph') {
      return MailSetupMethod.microsoftGraph;
    }
    return switch (MailOAuthProviderConfiguration.fromStorageName(
      account.oauthProvider,
    )) {
      MailOAuthProvider.google => MailSetupMethod.google,
      MailOAuthProvider.microsoftGraph => MailSetupMethod.microsoftGraph,
      MailOAuthProvider.microsoft365 || null => MailSetupMethod.microsoftImap,
    };
  }

  @override
  void dispose() {
    displayName.dispose();
    email.dispose();
    imapHost.dispose();
    imapPort.dispose();
    imapUsername.dispose();
    smtpHost.dispose();
    smtpPort.dispose();
    smtpUsername.dispose();
    password.dispose();
    super.dispose();
  }

  bool validateIdentity({
    required bool requirePassword,
    bool reportErrors = true,
  }) {
    final address = email.text.trim();
    final separator = address.lastIndexOf('@');
    final validAddress =
        separator > 0 &&
        separator == address.indexOf('@') &&
        separator < address.length - 1;
    if (displayName.text.trim().isNotEmpty &&
        validAddress &&
        (!requirePassword || password.text.isNotEmpty)) {
      return true;
    }
    if (reportErrors) {
      setState(() {
        status = requirePassword
            ? 'Bitte Kontoname, gültige E-Mail-Adresse und Passwort angeben.'
            : 'Bitte Kontoname und eine gültige E-Mail-Adresse angeben.';
        statusSuccess = false;
      });
    }
    return false;
  }

  MailAccountConfig? configuration({
    required bool requirePassword,
    bool reportErrors = true,
  }) {
    if (!validateIdentity(
      requirePassword: requirePassword,
      reportErrors: reportErrors,
    )) {
      return null;
    }
    final parsedImapPort = int.tryParse(imapPort.text.trim());
    final parsedSmtpPort = int.tryParse(smtpPort.text.trim());
    final requiredValues = [
      imapHost.text,
      imapUsername.text,
      smtpHost.text,
      smtpUsername.text,
    ];
    if (requiredValues.any((value) => value.trim().isEmpty) ||
        parsedImapPort == null ||
        parsedImapPort < 1 ||
        parsedImapPort > 65535 ||
        parsedSmtpPort == null ||
        parsedSmtpPort < 1 ||
        parsedSmtpPort > 65535) {
      if (reportErrors) {
        setState(() {
          status =
              'Bitte alle Serverfelder und gültige Ports zwischen 1 und 65535 angeben.';
          statusSuccess = false;
        });
      }
      return null;
    }
    final oauthProvider = method.oauthProvider;
    return MailAccountConfig(
      id: accountId,
      provider: method.mailProvider,
      displayName: displayName.text.trim(),
      email: email.text.trim(),
      imapHost: imapHost.text.trim(),
      imapPort: parsedImapPort,
      imapSecurity: imapSecurity,
      imapUsername: imapUsername.text.trim(),
      smtpHost: smtpHost.text.trim(),
      smtpPort: parsedSmtpPort,
      smtpSecurity: smtpSecurity,
      smtpUsername: smtpUsername.text.trim(),
      authentication: oauthProvider == null ? 'password' : 'oauth2',
      oauthProvider: oauthProvider?.storageName,
      aliases: widget.existing?.aliases ?? const [],
      lastSyncAt: widget.existing?.lastSyncAt,
    );
  }

  int configurationFingerprint(MailAccountConfig account) => Object.hashAll([
    account.provider,
    account.email,
    account.imapHost,
    account.imapPort,
    account.imapSecurity,
    account.imapUsername,
    account.smtpHost,
    account.smtpPort,
    account.smtpSecurity,
    account.smtpUsername,
    account.authentication,
    account.oauthProvider,
    password.text,
  ]);

  void applyOAuthProviderSettings(MailOAuthProvider provider) {
    final address = email.text.trim();
    imapUsername.text = address;
    smtpUsername.text = address;
    imapPort.text = '993';
    imapSecurity = 'tls';
    smtpPort.text = '587';
    smtpSecurity = 'starttls';
    switch (provider) {
      case MailOAuthProvider.microsoft365:
      case MailOAuthProvider.microsoftGraph:
        // Graph accounts keep the standards endpoints only as a documented
        // fallback description; synchronization runs through the Graph API.
        imapHost.text = 'outlook.office365.com';
        smtpHost.text = 'smtp.office365.com';
        break;
      case MailOAuthProvider.google:
        imapHost.text = 'imap.gmail.com';
        smtpHost.text = 'smtp.gmail.com';
        break;
    }
  }

  void applyDiscoveredSettings(DiscoveredMailSettings settings) {
    imapHost.text = settings.imapHost;
    imapPort.text = '${settings.imapPort}';
    imapSecurity = settings.imapSecurity;
    imapUsername.text = settings.imapUsername;
    smtpHost.text = settings.smtpHost;
    smtpPort.text = '${settings.smtpPort}';
    smtpSecurity = settings.smtpSecurity;
    smtpUsername.text = settings.smtpUsername;
  }

  void selectSuggestion(MailSetupSuggestion suggestion) {
    if (!suggestion.method.isSupported) return;
    setState(() {
      selected = suggestion;
      oauthTokens = null;
      testedConfiguration = null;
      status = null;
      statusSuccess = false;
      final provider = suggestion.method.oauthProvider;
      if (provider != null) {
        applyOAuthProviderSettings(provider);
      } else {
        final settings = suggestion.settings;
        if (settings != null) applyDiscoveredSettings(settings);
        final address = email.text.trim();
        if (imapUsername.text.trim().isEmpty) imapUsername.text = address;
        if (smtpUsername.text.trim().isEmpty) smtpUsername.text = address;
      }
    });
  }

  Future<void> continueToMethods() async {
    if (!validateIdentity(requirePassword: false)) return;
    final address = email.text.trim();
    setState(() {
      busy = true;
      statusSuccess = false;
      status = 'Anbieter für $address wird erkannt …';
    });
    MailSetupDetection result;
    try {
      result = await (widget.onDetect ?? detectMailSetup)(address);
    } on Object catch (error) {
      result = MailSetupDetection(
        emailAddress: address,
        suggestions: const [
          MailSetupSuggestion(
            method: MailSetupMethod.manual,
            recommended: true,
          ),
        ],
        summary:
            'Die automatische Erkennung war nicht möglich. Bitte die '
            'Servereinstellungen manuell eingeben.\n\n$error',
      );
    }
    if (!mounted) return;
    setState(() {
      busy = false;
      status = null;
      detection = result;
      step = _AccountSetupStep.method;
    });
    selectSuggestion(result.recommended);
  }

  void backToIdentity() {
    setState(() {
      step = _AccountSetupStep.identity;
      status = null;
      statusSuccess = false;
      oauthTokens = null;
      testedConfiguration = null;
    });
  }

  void editServersManually() {
    final current = selected;
    final candidates = current?.settingsCandidates ?? const [];
    final manual = detection?.suggestions.firstWhere(
      (suggestion) => suggestion.method == MailSetupMethod.manual,
      orElse: () => MailSetupSuggestion(
        method: MailSetupMethod.manual,
        settingsCandidates: candidates,
      ),
    );
    if (manual == null) return;
    selectSuggestion(
      MailSetupSuggestion(
        method: MailSetupMethod.manual,
        settingsCandidates: candidates.isEmpty
            ? manual.settingsCandidates
            : candidates,
      ),
    );
  }

  Future<MailOAuthTokens> authorizeOAuth(MailOAuthProvider provider) {
    final callback = widget.onAuthorizeOAuth;
    if (callback != null) {
      return callback(provider, email.text.trim());
    }
    return MailOAuthService().authorize(
      provider: provider,
      loginHint: email.text.trim(),
    );
  }

  Future<void> connectOAuth({bool closeAfterSuccess = false}) async {
    final provider = method.oauthProvider;
    if (provider == null || !validateIdentity(requirePassword: false)) return;
    setState(() {
      busy = true;
      statusSuccess = false;
      status = '${provider.displayName} wird im Browser geöffnet …';
      applyOAuthProviderSettings(provider);
    });
    var dialogClosed = false;
    try {
      final tokens = await authorizeOAuth(provider);
      if (!mounted) return;
      final account = configuration(requirePassword: false);
      if (account == null) return;
      final testOAuth = widget.onTestOAuth;
      if (testOAuth == null) {
        throw StateError('OAuth-Verbindungstest ist nicht verfügbar.');
      }
      setState(() {
        status = provider == MailOAuthProvider.microsoftGraph
            ? 'Anmeldung erfolgreich. Zugriff auf das Postfach wird geprüft …'
            : 'Anmeldung erfolgreich. Posteingang und Postausgang werden geprüft …';
      });
      await testOAuth(account, tokens);
      if (!mounted) return;
      oauthTokens = tokens;
      testedConfiguration = configurationFingerprint(account);
      setState(() {
        status = '${provider.displayName} wurde erfolgreich verbunden.';
        statusSuccess = true;
      });
      if (closeAfterSuccess) {
        dialogClosed = true;
        Navigator.pop(
          context,
          AccountSetupResult(account: account, oauthTokens: tokens),
        );
      }
    } on Object catch (error) {
      if (!mounted) return;
      setState(() {
        status = 'Anmeldung fehlgeschlagen: $error';
        statusSuccess = false;
      });
    } finally {
      if (mounted && !dialogClosed) setState(() => busy = false);
    }
  }

  /// Verifies the detected server candidates in order with the password.
  Future<MailAccountConfig?> testDetectedServers({
    required bool saveAfterSuccess,
  }) async {
    if (!validateIdentity(requirePassword: true)) return null;
    final candidates = selected?.settingsCandidates ?? const [];
    if (candidates.isEmpty) {
      setState(() {
        status =
            'Für diese Adresse wurden keine Servereinstellungen gefunden. '
            'Bitte die Server manuell eingeben.';
        statusSuccess = false;
      });
      return null;
    }
    setState(() {
      busy = true;
      statusSuccess = false;
    });
    var dialogClosed = false;
    try {
      Object? lastError;
      for (final settings in candidates.take(3)) {
        setState(() {
          applyDiscoveredSettings(settings);
          status =
              '${settings.imapHost} und ${settings.smtpHost} werden geprüft …';
        });
        final account = configuration(
          requirePassword: true,
          reportErrors: false,
        );
        if (account == null) continue;
        try {
          await widget.onTest(account, password.text);
          if (!mounted) return null;
          testedConfiguration = configurationFingerprint(account);
          setState(() {
            status = 'Verbindung erfolgreich geprüft.';
            statusSuccess = true;
          });
          if (saveAfterSuccess) {
            dialogClosed = true;
            Navigator.pop(
              context,
              AccountSetupResult(account: account, password: password.text),
            );
          }
          return account;
        } on Object catch (error) {
          lastError = error;
          final message = error.toString().toLowerCase();
          if (message.contains('authentication') ||
              message.contains('authentifizierung') ||
              message.contains('login') ||
              message.contains('anmeldung')) {
            break;
          }
        }
      }
      if (!mounted) return null;
      setState(() {
        status =
            'Die erkannten Server haben die Anmeldung nicht bestätigt. Bitte '
            'Passwort prüfen oder die Server manuell eingeben.\n\n$lastError';
        statusSuccess = false;
      });
    } finally {
      if (mounted && !dialogClosed) setState(() => busy = false);
    }
    return null;
  }

  Future<void> testManualServers() async {
    final account = configuration(requirePassword: true);
    if (account == null) return;
    setState(() {
      busy = true;
      status = 'Posteingang und Postausgang werden geprüft …';
      statusSuccess = false;
    });
    try {
      await widget.onTest(account, password.text);
      if (!mounted) return;
      testedConfiguration = configurationFingerprint(account);
      setState(() {
        status = 'Verbindung erfolgreich geprüft.';
        statusSuccess = true;
      });
    } on Object catch (error) {
      if (!mounted) return;
      setState(() {
        status = 'Verbindung fehlgeschlagen: $error';
        statusSuccess = false;
      });
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> testConnection() async {
    switch (method) {
      case MailSetupMethod.microsoftGraph:
      case MailSetupMethod.microsoftImap:
      case MailSetupMethod.google:
        await connectOAuth();
      case MailSetupMethod.imapPassword:
        await testDetectedServers(saveAfterSuccess: false);
      case MailSetupMethod.manual:
        await testManualServers();
      case MailSetupMethod.exchangeOnPremises:
        break;
    }
  }

  Future<void> save() async {
    final provider = method.oauthProvider;
    if (provider != null) {
      final account = configuration(requirePassword: false);
      if (account == null) return;
      if (oauthTokens != null &&
          testedConfiguration == configurationFingerprint(account)) {
        Navigator.pop(
          context,
          AccountSetupResult(account: account, oauthTokens: oauthTokens),
        );
        return;
      }
      if (widget.existing?.authentication == 'oauth2' &&
          widget.existing?.oauthProvider == provider.storageName &&
          widget.existing?.provider == account.provider) {
        Navigator.pop(context, AccountSetupResult(account: account));
        return;
      }
      await connectOAuth(closeAfterSuccess: true);
      return;
    }
    if (method == MailSetupMethod.imapPassword) {
      final configured = configuration(
        requirePassword: true,
        reportErrors: false,
      );
      if (configured != null &&
          testedConfiguration == configurationFingerprint(configured)) {
        Navigator.pop(
          context,
          AccountSetupResult(account: configured, password: password.text),
        );
        return;
      }
      await testDetectedServers(saveAfterSuccess: true);
      return;
    }
    final account = configuration(
      requirePassword:
          widget.existing == null ||
          widget.existing?.authentication == 'oauth2',
    );
    if (account == null) return;
    Navigator.pop(
      context,
      AccountSetupResult(account: account, password: password.text),
    );
  }

  @override
  Widget build(BuildContext context) {
    final identityStep = step == _AccountSetupStep.identity;
    return AlertDialog(
      title: Text(isEditing ? 'Konto bearbeiten' : 'E-Mail-Konto hinzufügen'),
      content: SizedBox(
        width: 650,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Flexible(
              child: SingleChildScrollView(
                child: identityStep ? buildIdentityStep() : buildMethodStep(),
              ),
            ),
            // Progress and status stay visible regardless of scroll position;
            // a hidden result reads as "nothing happened".
            if (busy) ...[
              const SizedBox(height: 10),
              const LinearProgressIndicator(),
            ],
            if (status != null) ...[
              const SizedBox(height: 10),
              Text(
                status!,
                key: const Key('account-status'),
                style: TextStyle(
                  fontSize: 12,
                  color: statusSuccess ? Colors.green.shade700 : null,
                ),
              ),
            ],
          ],
        ),
      ),
      actions: identityStep
          ? [
              TextButton(
                onPressed: busy ? null : () => Navigator.pop(context),
                child: const Text('Abbrechen'),
              ),
              FilledButton(
                key: const Key('account-continue'),
                onPressed: busy ? null : continueToMethods,
                child: const Text('Weiter'),
              ),
            ]
          : [
              TextButton(
                onPressed: busy ? null : () => Navigator.pop(context),
                child: const Text('Abbrechen'),
              ),
              if (!isEditing)
                TextButton(
                  key: const Key('account-back'),
                  onPressed: busy ? null : backToIdentity,
                  child: const Text('Zurück'),
                ),
              if (!method.usesOAuth || isEditing)
                OutlinedButton(
                  key: const Key('account-test'),
                  onPressed: busy ? null : testConnection,
                  child: Text(
                    method.usesOAuth
                        ? 'Anmelden und testen'
                        : 'Verbindung testen',
                  ),
                ),
              if (!method.usesOAuth || isEditing)
                FilledButton(
                  key: const Key('account-save'),
                  onPressed: busy ? null : save,
                  child: Text(isEditing ? 'Speichern' : 'Konto hinzufügen'),
                ),
            ],
    );
  }

  Widget buildIdentityStep() {
    return Column(
      key: const Key('account-identity-step'),
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text(
          'Gib deine E-Mail-Adresse ein. MAICENTA erkennt den passenden '
          'Zugang automatisch und schlägt ihn dir vor. Du kannst die Auswahl '
          'danach jederzeit ändern.',
          style: TextStyle(fontSize: 12),
        ),
        const SizedBox(height: 14),
        _AccountField(
          key: const Key('account-display-name'),
          label: 'Kontoname',
          controller: displayName,
        ),
        _AccountField(
          key: const Key('account-email'),
          label: 'E-Mail-Adresse',
          controller: email,
          onChanged: (value) {
            imapUsername.text = value;
            smtpUsername.text = value;
          },
        ),
      ],
    );
  }

  Widget buildMethodStep() {
    final detected = detection;
    final suggestions = detected?.suggestions ?? const <MailSetupSuggestion>[];
    return Column(
      key: const Key('account-method-step'),
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          email.text.trim(),
          style: const TextStyle(fontWeight: FontWeight.w600),
        ),
        if (detected != null) ...[
          const SizedBox(height: 2),
          Text(
            detected.summary,
            key: const Key('account-detection-summary'),
            style: const TextStyle(fontSize: 12),
          ),
        ],
        const SizedBox(height: 12),
        for (final suggestion in suggestions)
          if (suggestion.method.isSupported)
            _MethodChoice(
              key: Key('account-method-${suggestion.method.name}'),
              suggestion: suggestion,
              selected: selected?.method == suggestion.method,
              enabled: !busy,
              onSelected: () => selectSuggestion(suggestion),
            )
          else
            _MethodNotice(
              key: Key('account-method-${suggestion.method.name}'),
              suggestion: suggestion,
            ),
        const SizedBox(height: 10),
        ...buildMethodDetails(),
        const SizedBox(height: 6),
        ExpansionTile(
          key: const Key('account-advanced'),
          title: const Text('Erweitert', style: TextStyle(fontSize: 13)),
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(bottom: 8),
          initiallyExpanded: showAdvanced,
          onExpansionChanged: (expanded) =>
              setState(() => showAdvanced = expanded),
          children: [
            Align(
              alignment: Alignment.centerLeft,
              child: Text(
                method.technicalSummary,
                key: const Key('account-technical-summary'),
                style: const TextStyle(fontSize: 12),
              ),
            ),
            if (method == MailSetupMethod.imapPassword)
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton(
                  key: const Key('account-edit-servers'),
                  onPressed: busy ? null : editServersManually,
                  child: const Text('Server manuell anpassen'),
                ),
              ),
          ],
        ),
      ],
    );
  }

  List<Widget> buildMethodDetails() {
    final provider = method.oauthProvider;
    if (provider != null) {
      final label = provider.usesMicrosoftIdentity
          ? 'Mit Microsoft anmelden'
          : 'Mit Google anmelden';
      return [
        FilledButton.icon(
          key: const Key('account-oauth-login'),
          // Adding a new account: sign in, verify, and finish in one step.
          // Editing: only refresh the sign-in; Speichern closes the dialog.
          onPressed: busy
              ? null
              : () => connectOAuth(closeAfterSuccess: !isEditing),
          icon: const Icon(Icons.open_in_browser, size: 18),
          label: Text(
            widget.existing?.authentication == 'oauth2'
                ? 'Erneut im Browser anmelden'
                : label,
          ),
        ),
        const SizedBox(height: 6),
        Text(
          isEditing
              ? 'Die Anmeldung öffnet den Browser deines Systems. MAICENTA '
                    'sieht dein Passwort nie; es speichert nur die '
                    'Zugangsschlüssel im verschlüsselten Profil.'
              : 'Die Anmeldung öffnet den Browser deines Systems. Nach der '
                    'Bestätigung prüft MAICENTA den Zugriff und legt das Konto '
                    'an. Dein Passwort sieht MAICENTA nie; es speichert nur '
                    'die Zugangsschlüssel im verschlüsselten Profil.',
          style: const TextStyle(fontSize: 12),
        ),
      ];
    }
    if (method == MailSetupMethod.imapPassword) {
      final settings = selected?.settings;
      return [
        _AccountField(
          key: const Key('account-password'),
          label: 'Passwort deines E-Mail-Postfachs',
          controller: password,
          obscureText: true,
        ),
        if (settings != null)
          Text(
            'Posteingang ${settings.imapHost}, Postausgang ${settings.smtpHost} '
            '(${settings.source}). Das Passwort wird nur an diese Server '
            'gesendet und im verschlüsselten Profil gespeichert.',
            key: const Key('account-detected-servers'),
            style: const TextStyle(fontSize: 12),
          ),
      ];
    }
    return [
      _AccountField(
        key: const Key('account-password'),
        label: 'Passwort oder App-Passwort',
        controller: password,
        obscureText: true,
      ),
      if (isEditing)
        const Padding(
          padding: EdgeInsets.only(bottom: 8),
          child: Text(
            'Beim Bearbeiten kann das Passwort leer bleiben; das gespeicherte '
            'bleibt dann erhalten.',
            style: TextStyle(fontSize: 12),
          ),
        ),
      Column(
        key: const Key('account-manual-settings'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text(
            'Posteingang (IMAP)',
            style: TextStyle(fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 6),
          _ServerRow(
            hostKey: const Key('account-imap-host'),
            host: imapHost,
            port: imapPort,
            security: imapSecurity,
            onSecurityChanged: (value) => setState(() {
              imapSecurity = value;
              imapPort.text = value == 'tls' ? '993' : '143';
              testedConfiguration = null;
            }),
          ),
          _AccountField(label: 'IMAP-Benutzername', controller: imapUsername),
          const SizedBox(height: 10),
          const Text(
            'Postausgang (SMTP)',
            style: TextStyle(fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 6),
          _ServerRow(
            hostKey: const Key('account-smtp-host'),
            host: smtpHost,
            port: smtpPort,
            security: smtpSecurity,
            onSecurityChanged: (value) => setState(() {
              smtpSecurity = value;
              smtpPort.text = value == 'tls' ? '465' : '587';
              testedConfiguration = null;
            }),
          ),
          _AccountField(label: 'SMTP-Benutzername', controller: smtpUsername),
        ],
      ),
    ];
  }
}

class _MethodChoice extends StatelessWidget {
  const _MethodChoice({
    super.key,
    required this.suggestion,
    required this.selected,
    required this.enabled,
    required this.onSelected,
  });

  final MailSetupSuggestion suggestion;
  final bool selected;
  final bool enabled;
  final VoidCallback onSelected;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: InkWell(
        onTap: enabled ? onSelected : null,
        borderRadius: BorderRadius.circular(4),
        child: Container(
          padding: const EdgeInsets.fromLTRB(6, 8, 12, 8),
          decoration: BoxDecoration(
            border: Border.all(
              color: selected ? scheme.primary : scheme.outlineVariant,
              width: selected ? 1.5 : 1,
            ),
            borderRadius: BorderRadius.circular(4),
            color: selected
                ? scheme.primaryContainer.withValues(alpha: 0.35)
                : null,
          ),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              IgnorePointer(
                child: Radio<bool>(
                  value: true,
                  // ignore: deprecated_member_use
                  groupValue: selected,
                  // ignore: deprecated_member_use
                  onChanged: (_) {},
                ),
              ),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Wrap(
                      spacing: 8,
                      runSpacing: 2,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Text(
                          suggestion.method.title,
                          style: const TextStyle(fontWeight: FontWeight.w600),
                        ),
                        if (suggestion.recommended)
                          Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 6,
                              vertical: 1,
                            ),
                            decoration: BoxDecoration(
                              color: scheme.primary,
                              borderRadius: BorderRadius.circular(3),
                            ),
                            child: Text(
                              'Empfohlen',
                              style: TextStyle(
                                fontSize: 11,
                                color: scheme.onPrimary,
                              ),
                            ),
                          ),
                      ],
                    ),
                    const SizedBox(height: 2),
                    Text(
                      suggestion.method.description,
                      style: const TextStyle(fontSize: 12),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _MethodNotice extends StatelessWidget {
  const _MethodNotice({super.key, required this.suggestion});

  final MailSetupSuggestion suggestion;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 6),
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(4),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Icon(Icons.info_outline, size: 18),
          const SizedBox(width: 8),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  suggestion.method.title,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                Text(
                  suggestion.method.description,
                  style: const TextStyle(fontSize: 12),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _AccountField extends StatelessWidget {
  const _AccountField({
    super.key,
    required this.label,
    required this.controller,
    this.obscureText = false,
    this.onChanged,
  });

  final String label;
  final TextEditingController controller;
  final bool obscureText;
  final ValueChanged<String>? onChanged;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 7),
      child: TextField(
        controller: controller,
        obscureText: obscureText,
        onChanged: onChanged,
        decoration: InputDecoration(
          labelText: label,
          border: const OutlineInputBorder(),
        ),
      ),
    );
  }
}

class _ServerRow extends StatelessWidget {
  const _ServerRow({
    required this.hostKey,
    required this.host,
    required this.port,
    required this.security,
    required this.onSecurityChanged,
  });

  final Key hostKey;
  final TextEditingController host;
  final TextEditingController port;
  final String security;
  final ValueChanged<String> onSecurityChanged;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: _AccountField(key: hostKey, label: 'Server', controller: host),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 90,
          child: _AccountField(label: 'Port', controller: port),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 180,
          child: DropdownButtonFormField<String>(
            initialValue: security,
            isExpanded: true,
            decoration: const InputDecoration(
              labelText: 'Sicherheit',
              border: OutlineInputBorder(),
            ),
            items: const [
              DropdownMenuItem(value: 'tls', child: Text('TLS')),
              DropdownMenuItem(value: 'starttls', child: Text('STARTTLS')),
            ],
            onChanged: (value) {
              if (value != null) onSecurityChanged(value);
            },
          ),
        ),
      ],
    );
  }
}

class AppTitleBar extends StatelessWidget {
  const AppTitleBar({
    super.key,
    this.moduleLabel = 'E-Mail',
    required this.onSearch,
    required this.searching,
    required this.searchActive,
    required this.includesContent,
    required this.onIncludeContent,
    required this.onSynchronize,
    required this.onNotifications,
    required this.onSettings,
    required this.onAppMenu,
    required this.onProfile,
  });

  /// Name of the active module shown next to the application name.
  final String moduleLabel;
  final ValueChanged<String> onSearch;
  final bool searching;
  final bool searchActive;
  final bool includesContent;
  final VoidCallback onIncludeContent;
  final VoidCallback onSynchronize;
  final VoidCallback onNotifications;
  final VoidCallback onSettings;
  final VoidCallback onAppMenu;
  final VoidCallback onProfile;

  @override
  Widget build(BuildContext context) {
    return Container(
      key: const Key('classic-title-bar'),
      height: 43,
      padding: const EdgeInsets.symmetric(horizontal: 9),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          Tooltip(
            message: 'MAICENTA-Module',
            child: InkWell(
              key: const Key('app-menu'),
              onTap: onAppMenu,
              child: Container(
                width: 28,
                height: 28,
                padding: const EdgeInsets.all(3),
                child: Image.asset(
                  maicentaSymbolAsset,
                  key: const Key('title-brand-symbol'),
                  fit: BoxFit.contain,
                  filterQuality: FilterQuality.high,
                  semanticLabel: 'MAICENTA-Logo',
                ),
              ),
            ),
          ),
          _TitleIcon(
            icon: Icons.sync,
            label: 'Alle Ordner synchronisieren',
            onPressed: onSynchronize,
          ),
          const SizedBox(width: 4),
          Text(
            'MAICENTA',
            style: TextStyle(
              color: Theme.of(context).colorScheme.onSurface,
              fontSize: 12,
              fontWeight: FontWeight.w600,
            ),
          ),
          Text(
            '  –  $moduleLabel',
            style: TextStyle(
              color: MaicentaPalette.of(context).mutedText,
              fontSize: 12,
            ),
          ),
          const SizedBox(width: 18),
          Expanded(
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 540),
                child: SizedBox(
                  height: 31,
                  child: TextField(
                    key: const Key('global-search'),
                    onChanged: onSearch,
                    style: const TextStyle(fontSize: 12),
                    decoration: InputDecoration(
                      hintText: includesContent
                          ? 'Betreff, Personen, Inhalte und Anhänge durchsuchen'
                          : 'Betreff, Absender oder Empfänger durchsuchen',
                      prefixIcon: const Icon(Icons.search, size: 17),
                      prefixIconConstraints: const BoxConstraints(minWidth: 34),
                      suffixIcon: searching
                          ? const Padding(
                              padding: EdgeInsets.all(8),
                              child: SizedBox.square(
                                dimension: 13,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                            )
                          : searchActive
                          ? IconButton(
                              key: const Key('include-message-content-search'),
                              tooltip: includesContent
                                  ? 'Mailtexte und Anhangsnamen werden durchsucht'
                                  : 'Auch Mailtexte und Anhangsnamen durchsuchen',
                              onPressed: includesContent
                                  ? null
                                  : onIncludeContent,
                              icon: Icon(
                                includesContent
                                    ? Icons.manage_search
                                    : Icons.description_outlined,
                                size: 17,
                              ),
                            )
                          : null,
                      filled: true,
                      fillColor: MaicentaPalette.of(context).input,
                      contentPadding: const EdgeInsets.symmetric(vertical: 5),
                      enabledBorder: OutlineInputBorder(
                        borderRadius: BorderRadius.zero,
                        borderSide: BorderSide(
                          color: MaicentaPalette.of(context).border,
                        ),
                      ),
                      focusedBorder: const OutlineInputBorder(
                        borderRadius: BorderRadius.zero,
                        borderSide: BorderSide(color: MaicentaApp.primaryBlue),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(width: 12),
          _TitleIcon(
            icon: Icons.notifications_none,
            label: 'Benachrichtigungen',
            onPressed: onNotifications,
          ),
          _TitleIcon(
            icon: Icons.settings_outlined,
            label: 'Einstellungen',
            onPressed: onSettings,
          ),
          const SizedBox(width: 8),
          Tooltip(
            message: 'Lokales Profil',
            child: InkWell(
              key: const Key('profile-menu'),
              onTap: onProfile,
              borderRadius: BorderRadius.circular(2),
              child: CircleAvatar(
                radius: 13,
                backgroundColor: MaicentaPalette.of(context).selected,
                child: const Text(
                  'MT',
                  style: TextStyle(
                    color: MaicentaApp.primaryBlue,
                    fontSize: 11,
                    fontWeight: FontWeight.bold,
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _TitleIcon extends StatelessWidget {
  const _TitleIcon({
    required this.icon,
    required this.label,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: label,
      child: IconButton(
        onPressed: onPressed,
        visualDensity: VisualDensity.compact,
        icon: Icon(
          icon,
          color: Theme.of(context).colorScheme.onSurface,
          size: 18,
        ),
      ),
    );
  }
}

class Ribbon extends StatefulWidget {
  const Ribbon({
    super.key,
    required this.module,
    required this.commands,
    required this.offlineMode,
    required this.selectedIsDraft,
  });

  final WorkspaceModule module;
  final RibbonCommands commands;
  final bool offlineMode;
  final bool selectedIsDraft;

  @override
  State<Ribbon> createState() => _RibbonState();
}

class _RibbonState extends State<Ribbon> {
  String selectedTab = 'Start';

  static const tabs = [
    'Datei',
    'Start',
    'Senden/Empfangen',
    'Ordner',
    'Ansicht',
  ];

  @override
  Widget build(BuildContext context) {
    final newLabel = switch (widget.module) {
      WorkspaceModule.mail => 'Neue E-Mail',
      WorkspaceModule.calendar => 'Neuer Termin',
      WorkspaceModule.tasks => 'Neue Aufgabe',
      WorkspaceModule.contacts => 'Neuer Kontakt',
    };

    return Container(
      key: const Key('classic-ribbon'),
      height: 106,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Column(
        children: [
          Container(
            height: 31,
            padding: const EdgeInsets.only(left: 6),
            decoration: BoxDecoration(
              color: MaicentaPalette.of(context).window,
              border: Border(
                bottom: BorderSide(color: MaicentaPalette.of(context).border),
              ),
            ),
            child: Row(
              children: [
                for (final tab in tabs)
                  InkWell(
                    key: Key('ribbon-tab-$tab'),
                    onTap: () => setState(() => selectedTab = tab),
                    child: Container(
                      height: 31,
                      padding: const EdgeInsets.symmetric(horizontal: 13),
                      alignment: Alignment.center,
                      decoration: BoxDecoration(
                        border: Border(
                          bottom: BorderSide(
                            color: selectedTab == tab
                                ? MaicentaApp.primaryBlue
                                : Colors.transparent,
                            width: 2,
                          ),
                        ),
                      ),
                      child: Text(
                        tab,
                        style: TextStyle(
                          fontSize: 12,
                          color: tab == 'Datei'
                              ? MaicentaApp.primaryBlue
                              : Theme.of(context).colorScheme.onSurface,
                          fontWeight: selectedTab == tab
                              ? FontWeight.w600
                              : FontWeight.normal,
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.fromLTRB(7, 3, 7, 2),
              child: ribbonActions(newLabel),
            ),
          ),
        ],
      ),
    );
  }

  Widget ribbonActions(String newLabel) {
    if (widget.module != WorkspaceModule.mail && selectedTab != 'Datei') {
      return Row(
        children: [
          _RibbonGroup(label: 'Neu', children: [_newItemButton(newLabel)]),
          _RibbonGroup(
            label: 'Aktionen',
            children: [
              RibbonAction(
                icon: Icons.refresh,
                label: 'Aktualisieren',
                onTap: widget.commands.synchronize,
              ),
              RibbonAction(
                icon: Icons.settings_outlined,
                label: 'Optionen',
                onTap: widget.commands.options,
              ),
            ],
          ),
        ],
      );
    }
    if (selectedTab == 'Senden/Empfangen') {
      return Row(
        children: [
          _RibbonGroup(
            label: 'Senden und Empfangen',
            children: [
              RibbonAction(
                icon: Icons.sync,
                label: 'Alle Ordner',
                onTap: widget.commands.synchronize,
              ),
              RibbonAction(
                icon: Icons.refresh,
                label: 'Ordner aktualisieren',
                onTap: widget.commands.synchronize,
              ),
            ],
          ),
          _RibbonGroup(
            label: 'Verbindung',
            children: [
              RibbonAction(
                icon: widget.offlineMode
                    ? Icons.cloud_done_outlined
                    : Icons.cloud_off_outlined,
                label: widget.offlineMode
                    ? 'Online arbeiten'
                    : 'Offline arbeiten',
                onTap: widget.commands.toggleOffline,
              ),
              RibbonAction(
                icon: Icons.download_outlined,
                label: 'Fortschritt anzeigen',
                onTap: widget.commands.showProgress,
              ),
            ],
          ),
        ],
      );
    }
    if (selectedTab == 'Ordner') {
      return Row(
        children: [
          _RibbonGroup(
            label: 'Neu',
            children: [
              RibbonAction(
                icon: Icons.create_new_folder_outlined,
                label: 'Neuer Ordner',
                onTap: widget.commands.newFolder,
              ),
            ],
          ),
          _RibbonGroup(
            label: 'Ordneraktionen',
            children: [
              RibbonAction(
                icon: Icons.drive_file_rename_outline,
                label: 'Umbenennen',
                onTap: widget.commands.renameFolder,
              ),
              RibbonAction(
                icon: Icons.delete_outline,
                label: 'Ordner löschen',
                onTap: widget.commands.deleteFolder,
              ),
              RibbonAction(
                icon: Icons.mark_email_read_outlined,
                label: 'Alle als gelesen',
                onTap: widget.commands.markAllRead,
              ),
            ],
          ),
        ],
      );
    }
    if (selectedTab == 'Ansicht') {
      return Row(
        children: [
          _RibbonGroup(
            label: 'Layout',
            children: [
              RibbonAction(
                icon: Icons.view_column_outlined,
                label: 'Ordnerbereich',
                onTap: widget.commands.toggleFolderPane,
              ),
              RibbonAction(
                icon: Icons.chrome_reader_mode_outlined,
                label: 'Lesebereich',
                onTap: widget.commands.toggleReadingPane,
              ),
            ],
          ),
          _RibbonGroup(
            label: 'Aktuelle Ansicht',
            children: [
              RibbonAction(
                icon: Icons.sort,
                label: 'Sortieren',
                onTap: widget.commands.cycleSort,
              ),
              RibbonAction(
                icon: Icons.zoom_in,
                label: 'Zoom',
                onTap: widget.commands.cycleZoom,
              ),
            ],
          ),
        ],
      );
    }
    if (selectedTab == 'Datei') {
      return Row(
        children: [
          _RibbonGroup(
            label: 'Profil',
            children: [
              RibbonAction(
                icon: Icons.manage_accounts_outlined,
                label: 'Kontoeinstellungen',
                onTap: widget.commands.accountSettings,
              ),
              RibbonAction(
                icon: Icons.archive_outlined,
                label: 'Import/Export',
                onTap: widget.commands.importExport,
              ),
            ],
          ),
          _RibbonGroup(
            label: 'Anwendung',
            children: [
              RibbonAction(
                icon: Icons.settings_outlined,
                label: 'Optionen',
                onTap: widget.commands.options,
              ),
            ],
          ),
        ],
      );
    }
    return Row(
      children: [
        _RibbonGroup(label: 'Neu', children: [_newItemButton(newLabel)]),
        _RibbonGroup(
          label: widget.selectedIsDraft ? 'Entwurf' : 'Antworten',
          children: [
            if (widget.selectedIsDraft)
              RibbonAction(
                icon: Icons.edit_outlined,
                label: 'Entwurf bearbeiten',
                onTap: widget.commands.editDraft,
              )
            else ...[
              RibbonAction(
                icon: Icons.reply,
                label: 'Antworten',
                onTap: widget.commands.reply,
              ),
              RibbonAction(
                icon: Icons.reply_all,
                label: 'Allen antworten',
                onTap: widget.commands.reply,
              ),
              RibbonAction(
                icon: Icons.forward,
                label: 'Weiterleiten',
                onTap: widget.commands.forward,
              ),
            ],
          ],
        ),
        _RibbonGroup(
          label: 'Löschen',
          children: [
            if (!widget.selectedIsDraft)
              RibbonAction(
                icon: Icons.archive_outlined,
                label: 'Archivieren',
                onTap: widget.commands.archive,
              ),
            RibbonAction(
              icon: Icons.delete_outline,
              label: 'Löschen',
              onTap: widget.commands.delete,
            ),
          ],
        ),
        if (!widget.selectedIsDraft)
          _RibbonGroup(
            label: 'Kategorien',
            children: [
              RibbonAction(
                icon: Icons.flag_outlined,
                label: 'Nachverfolgen',
                onTap: widget.commands.toggleFlag,
              ),
            ],
          ),
        _RibbonGroup(
          label: 'Senden/Empfangen',
          children: [
            RibbonAction(
              icon: Icons.sync,
              label: 'Synchronisieren',
              onTap: widget.commands.synchronize,
            ),
          ],
        ),
      ],
    );
  }

  Widget _newItemButton(String label) {
    return RibbonAction(
      key: const Key('new-item-button'),
      icon: switch (widget.module) {
        WorkspaceModule.mail => Icons.mail_outline,
        WorkspaceModule.calendar => Icons.event_outlined,
        WorkspaceModule.tasks => Icons.add_task_outlined,
        WorkspaceModule.contacts => Icons.person_add_outlined,
      },
      label: label,
      onTap: widget.commands.newItem,
    );
  }
}

class _RibbonGroup extends StatelessWidget {
  const _RibbonGroup({required this.label, required this.children});

  final String label;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 69,
      padding: const EdgeInsets.symmetric(horizontal: 4),
      decoration: BoxDecoration(
        border: Border(
          right: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Column(
        children: [
          Expanded(child: Row(children: children)),
          Text(
            label,
            style: TextStyle(
              fontSize: 9,
              color: MaicentaPalette.of(context).mutedText,
            ),
          ),
        ],
      ),
    );
  }
}

class RibbonAction extends StatelessWidget {
  const RibbonAction({
    super.key,
    required this.icon,
    required this.label,
    this.onTap,
  });

  final IconData icon;
  final String label;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return Opacity(
      opacity: onTap == null ? 0.45 : 1,
      child: InkWell(
        key: key == null ? Key('ribbon-action-$label') : null,
        onTap: onTap,
        borderRadius: BorderRadius.circular(2),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minWidth: 58, maxWidth: 105),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Icon(
                  icon,
                  size: 21,
                  color: Theme.of(context).colorScheme.onSurface,
                ),
                const SizedBox(height: 3),
                Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 10.5),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class ModuleRail extends StatelessWidget {
  const ModuleRail({
    super.key,
    required this.selected,
    required this.onSelected,
    required this.onMoreApps,
  });

  final WorkspaceModule selected;
  final ValueChanged<WorkspaceModule> onSelected;
  final VoidCallback onMoreApps;

  @override
  Widget build(BuildContext context) {
    return Container(
      key: const Key('classic-module-bar'),
      height: 41,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          top: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          ModuleButton(
            icon: Icons.mail_outline,
            label: 'E-Mail',
            selected: selected == WorkspaceModule.mail,
            onTap: () => onSelected(WorkspaceModule.mail),
          ),
          ModuleButton(
            icon: Icons.calendar_month_outlined,
            label: 'Kalender',
            selected: selected == WorkspaceModule.calendar,
            onTap: () => onSelected(WorkspaceModule.calendar),
          ),
          ModuleButton(
            icon: Icons.check_box_outlined,
            label: 'Aufgaben',
            selected: selected == WorkspaceModule.tasks,
            onTap: () => onSelected(WorkspaceModule.tasks),
          ),
          ModuleButton(
            icon: Icons.people_outline,
            label: 'Kontakte',
            selected: selected == WorkspaceModule.contacts,
            onTap: () => onSelected(WorkspaceModule.contacts),
          ),
          const Spacer(),
          ModuleButton(
            icon: Icons.apps_outlined,
            label: 'Weitere Apps',
            onTap: onMoreApps,
          ),
          const SizedBox(width: 3),
        ],
      ),
    );
  }
}

class ModuleButton extends StatelessWidget {
  const ModuleButton({
    super.key,
    required this.icon,
    required this.label,
    this.selected = false,
    this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: label,
      child: InkWell(
        key: Key('module-$label'),
        onTap: onTap,
        child: Container(
          width: 43,
          height: 41,
          decoration: BoxDecoration(
            color: selected
                ? MaicentaPalette.of(context).window
                : Colors.transparent,
            border: Border(
              bottom: BorderSide(
                color: selected ? MaicentaApp.primaryBlue : Colors.transparent,
                width: 2,
              ),
            ),
          ),
          child: Icon(
            icon,
            size: 19,
            color: selected
                ? Theme.of(context).colorScheme.primary
                : MaicentaPalette.of(context).mutedText,
          ),
        ),
      ),
    );
  }
}

class ClassicModuleWorkspace extends StatelessWidget {
  const ClassicModuleWorkspace({
    super.key,
    required this.module,
    required this.onModuleSelected,
    required this.onMoreApps,
    required this.child,
    this.sidebar,
  });

  final WorkspaceModule module;
  final ValueChanged<WorkspaceModule> onModuleSelected;
  final VoidCallback onMoreApps;
  final Widget child;

  /// Replaces the default single entry in the navigation pane.
  final Widget? sidebar;

  @override
  Widget build(BuildContext context) {
    final (title, icon, itemLabel) = switch (module) {
      WorkspaceModule.mail => ('E-Mail', Icons.mail_outline, 'Posteingang'),
      WorkspaceModule.calendar => (
        'Kalender',
        Icons.calendar_month_outlined,
        'Mein Kalender',
      ),
      WorkspaceModule.tasks => (
        'Aufgaben',
        Icons.check_box_outlined,
        'Meine Aufgaben',
      ),
      WorkspaceModule.contacts => (
        'Kontakte',
        Icons.people_outline,
        'Meine Kontakte',
      ),
    };
    return Row(
      children: [
        Container(
          width: 244,
          color: MaicentaPalette.of(context).pane,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(
                height: 45,
                padding: const EdgeInsets.symmetric(horizontal: 14),
                alignment: Alignment.centerLeft,
                decoration: BoxDecoration(
                  border: Border(
                    bottom: BorderSide(
                      color: MaicentaPalette.of(context).border,
                    ),
                  ),
                ),
                child: Text(
                  title,
                  style: const TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              if (sidebar case final sidebar?)
                Expanded(child: SingleChildScrollView(child: sidebar))
              else ...[
                Padding(
                  padding: const EdgeInsets.fromLTRB(13, 14, 10, 5),
                  child: Text(
                    title.toUpperCase(),
                    style: TextStyle(
                      fontSize: 10,
                      fontWeight: FontWeight.w600,
                      color: MaicentaPalette.of(context).mutedText,
                    ),
                  ),
                ),
                Container(
                  height: 29,
                  padding: const EdgeInsets.symmetric(horizontal: 17),
                  color: MaicentaPalette.of(context).selected,
                  child: Row(
                    children: [
                      Icon(icon, size: 16, color: MaicentaApp.primaryBlue),
                      const SizedBox(width: 8),
                      Text(
                        itemLabel,
                        style: const TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ],
                  ),
                ),
                const Spacer(),
              ],
              ModuleRail(
                selected: module,
                onSelected: onModuleSelected,
                onMoreApps: onMoreApps,
              ),
            ],
          ),
        ),
        const VerticalDivider(width: 1),
        Expanded(child: child),
      ],
    );
  }
}

class MailWorkspace extends StatelessWidget {
  const MailWorkspace({
    super.key,
    required this.messages,
    required this.folders,
    required this.accounts,
    required this.favoriteFolderIds,
    required this.collapsedFolderIds,
    required this.onToggleFolderCollapsed,
    required this.folderOrderIds,
    required this.onFolderOrderChanged,
    required this.selectedMessage,
    required this.selectedFolder,
    required this.onMessageSelected,
    required this.onMessageContextSelected,
    required this.onMessageOpened,
    required this.onMessageContextAction,
    required this.onFolderSelected,
    required this.onMessageDropped,
    required this.onFavoriteFolderOrderChanged,
    required this.totalMessageCount,
    required this.hasMoreMessages,
    required this.loadingMoreMessages,
    required this.onLoadMoreMessages,
    required this.onReply,
    required this.onForward,
    required this.onEditDraft,
    required this.onSaveAttachment,
    required this.onReloadContent,
    required this.onAccountSettings,
    required this.onNewFolder,
    required this.filter,
    required this.onFilterChanged,
    required this.showFolderPane,
    required this.showReadingPane,
    required this.readingZoom,
    required this.module,
    required this.onModuleSelected,
    required this.onMoreApps,
  });

  final List<DemoMessage> messages;
  final List<MailFolder> folders;
  final List<MailAccountConfig> accounts;
  final List<String> favoriteFolderIds;
  final Set<String> collapsedFolderIds;
  final ValueChanged<String> onToggleFolderCollapsed;
  final List<String> folderOrderIds;
  final ValueChanged<List<String>> onFolderOrderChanged;
  final int selectedMessage;
  final String selectedFolder;
  final ValueChanged<int> onMessageSelected;
  final ValueChanged<int> onMessageContextSelected;
  final ValueChanged<int> onMessageOpened;
  final Future<void> Function(DemoMessage message, MailContextAction action)
  onMessageContextAction;
  final ValueChanged<String> onFolderSelected;
  final void Function(DemoMessage message, MailFolder folder) onMessageDropped;
  final ValueChanged<List<String>> onFavoriteFolderOrderChanged;
  final int totalMessageCount;
  final bool hasMoreMessages;
  final bool loadingMoreMessages;
  final VoidCallback onLoadMoreMessages;
  final VoidCallback onReply;
  final VoidCallback onForward;
  final VoidCallback onEditDraft;
  final ValueChanged<MailAttachmentData> onSaveAttachment;
  final Future<DemoMessage?> Function(DemoMessage message) onReloadContent;
  final VoidCallback onAccountSettings;
  final VoidCallback onNewFolder;
  final MailListFilter filter;
  final ValueChanged<MailListFilter> onFilterChanged;
  final bool showFolderPane;
  final bool showReadingPane;
  final double readingZoom;
  final WorkspaceModule module;
  final ValueChanged<WorkspaceModule> onModuleSelected;
  final VoidCallback onMoreApps;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 900;
        final folderPaneShown = !compact && showFolderPane;
        final selected = messages.isEmpty
            ? null
            : messages[selectedMessage.clamp(0, messages.length - 1)];

        return Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (folderPaneShown)
              FolderPane(
                folders: folders,
                accounts: accounts,
                favoriteFolderIds: favoriteFolderIds,
                collapsedFolderIds: collapsedFolderIds,
                onToggleFolderCollapsed: onToggleFolderCollapsed,
                folderOrderIds: folderOrderIds,
                onFolderOrderChanged: onFolderOrderChanged,
                selectedFolder: selectedFolder,
                onSelected: onFolderSelected,
                onMessageDropped: onMessageDropped,
                onFavoriteFolderOrderChanged: onFavoriteFolderOrderChanged,
                onAccountSettings: onAccountSettings,
                onNewFolder: onNewFolder,
                module: module,
                onModuleSelected: onModuleSelected,
                onMoreApps: onMoreApps,
              ),
            if (folderPaneShown) const VerticalDivider(width: 1),
            SizedBox(
              width: showReadingPane
                  ? compact
                        ? constraints.maxWidth * 0.44
                        : 355
                  : constraints.maxWidth - (folderPaneShown ? 245 : 0),
              child: MessageList(
                messages: messages,
                folders: folders,
                selectedIndex: selectedMessage,
                folder: folderDisplayName(context, folders, selectedFolder),
                totalMessageCount: totalMessageCount,
                hasMore: hasMoreMessages,
                loadingMore: loadingMoreMessages,
                onLoadMore: onLoadMoreMessages,
                onSelected: onMessageSelected,
                onContextSelected: onMessageContextSelected,
                onOpened: onMessageOpened,
                onContextAction: onMessageContextAction,
                onMoved: onMessageDropped,
                filter: filter,
                onFilterChanged: onFilterChanged,
              ),
            ),
            if (showReadingPane) const VerticalDivider(width: 1),
            if (showReadingPane)
              Expanded(
                child: ReadingPane(
                  message: selected,
                  onReply: onReply,
                  onForward: onForward,
                  onEditDraft: onEditDraft,
                  onSaveAttachment: onSaveAttachment,
                  onReloadContent: onReloadContent,
                  zoom: readingZoom,
                ),
              ),
          ],
        );
      },
    );
  }
}

/// Payload of a folder drag: the folder and whether the drag started in the
/// favorites list, which adds or reorders favorites but never rearranges the
/// account tree.
class FolderDragData {
  const FolderDragData(this.folder, {required this.fromFavorites});

  final MailFolder folder;
  final bool fromFavorites;
}

class FolderPane extends StatelessWidget {
  const FolderPane({
    super.key,
    required this.folders,
    required this.accounts,
    required this.favoriteFolderIds,
    this.collapsedFolderIds = const {},
    this.onToggleFolderCollapsed,
    this.folderOrderIds = const [],
    this.onFolderOrderChanged,
    required this.selectedFolder,
    required this.onSelected,
    required this.onMessageDropped,
    required this.onFavoriteFolderOrderChanged,
    required this.onAccountSettings,
    required this.onNewFolder,
    required this.module,
    required this.onModuleSelected,
    required this.onMoreApps,
  });

  final List<MailFolder> folders;
  final List<MailAccountConfig> accounts;
  final List<String> favoriteFolderIds;
  final Set<String> collapsedFolderIds;
  final ValueChanged<String>? onToggleFolderCollapsed;

  /// User-defined sibling order, see [buildFolderTree].
  final List<String> folderOrderIds;

  /// Receives the complete new order after a folder was dropped onto a
  /// sibling; `null` disables rearranging.
  final ValueChanged<List<String>>? onFolderOrderChanged;
  final String selectedFolder;
  final ValueChanged<String> onSelected;
  final void Function(DemoMessage message, MailFolder folder) onMessageDropped;
  final ValueChanged<List<String>> onFavoriteFolderOrderChanged;
  final VoidCallback onAccountSettings;
  final VoidCallback onNewFolder;
  final WorkspaceModule module;
  final ValueChanged<WorkspaceModule> onModuleSelected;
  final VoidCallback onMoreApps;

  @override
  Widget build(BuildContext context) {
    final localizations = AppLocalizations.of(context);
    final accountLabels = <String, String>{
      'personal': localizations.localArea,
      for (final account in accounts) account.id: account.displayName,
    };
    final groupIds = <String>['personal'];
    for (final account in accounts) {
      if (!groupIds.contains(account.id)) groupIds.add(account.id);
    }
    for (final folder in folders) {
      if (!groupIds.contains(folder.accountId)) groupIds.add(folder.accountId);
    }
    final connectionLabel = accounts.isEmpty
        ? localizations.noMailAccountConnected
        : localizations.mailAccountsConnected(accounts.length);
    // Group headers share the collapsed-folder preference under a reserved
    // `group.` prefix so the whole pane state survives a restart together.
    const favoritesGroupId = 'group.favorites';
    final favoritesCollapsed = collapsedFolderIds.contains(favoritesGroupId);
    bool accountCollapsed(String groupId) =>
        collapsedFolderIds.contains('group.$groupId');
    final foldersById = {for (final folder in folders) folder.id: folder};
    final favoriteFolders = favoriteFolderIds
        .map((id) => foldersById[id])
        .whereType<MailFolder>()
        .toList(growable: false);
    final favoriteLabelCounts = <String, int>{};
    for (final folder in favoriteFolders) {
      final label = mailboxDisplayName(context, folder).trim().toLowerCase();
      favoriteLabelCounts[label] = (favoriteLabelCounts[label] ?? 0) + 1;
    }

    return Container(
      key: const Key('classic-folder-pane'),
      width: 244,
      color: MaicentaPalette.of(context).pane,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            height: 37,
            padding: const EdgeInsets.symmetric(horizontal: 13),
            alignment: Alignment.centerLeft,
            decoration: BoxDecoration(
              border: Border(
                bottom: BorderSide(color: MaicentaPalette.of(context).border),
              ),
            ),
            child: Text(
              localizations.mailModule,
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
            ),
          ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.only(top: 5),
              children: [
                DragTarget<FolderDragData>(
                  key: const Key('favorite-drop-zone'),
                  onWillAcceptWithDetails: (_) => true,
                  onAcceptWithDetails: (details) {
                    final reordered = favoriteFolderIds
                        .where((id) => id != details.data.folder.id)
                        .toList();
                    reordered.add(details.data.folder.id);
                    onFavoriteFolderOrderChanged(reordered);
                  },
                  builder: (context, candidates, _) => DecoratedBox(
                    decoration: BoxDecoration(
                      color: candidates.isEmpty
                          ? Colors.transparent
                          : MaicentaPalette.of(context).selected,
                    ),
                    child: _FolderGroupLabel(
                      label: localizations.favorites,
                      trailing: candidates.isEmpty ? null : Icons.add,
                      collapsed: favoritesCollapsed,
                      toggleKey: const Key('group-toggle-favorites'),
                      onToggle: onToggleFolderCollapsed == null
                          ? null
                          : () => onToggleFolderCollapsed!(favoritesGroupId),
                    ),
                  ),
                ),
                if (!favoritesCollapsed) ...[
                  for (final folder in favoriteFolders)
                    _favoriteFolderTile(
                      context,
                      folder: folder,
                      secondaryLabel:
                          favoriteLabelCounts[mailboxDisplayName(
                                context,
                                folder,
                              ).trim().toLowerCase()]! >
                              1
                          ? _accountQualifier(folder, accountLabels)
                          : null,
                    ),
                  FolderTile(
                    id: 'virtual.unread',
                    label: localizations.unreadEmails,
                    icon: Icons.mark_email_unread_outlined,
                    count: folders.fold<int>(
                      0,
                      (count, folder) =>
                          count +
                          (folder.role == 'drafts' ? 0 : folder.unreadCount),
                    ),
                    selected: selectedFolder == 'virtual.unread',
                    onTap: onSelected,
                  ),
                  FolderTile(
                    id: 'virtual.flagged',
                    label: localizations.followUp,
                    icon: Icons.flag_outlined,
                    selected: selectedFolder == 'virtual.flagged',
                    onTap: onSelected,
                  ),
                ],
                const Divider(height: 11, indent: 10, endIndent: 10),
                for (final groupId in groupIds) ...[
                  DragTarget<FolderDragData>(
                    key: Key('favorite-remove-account-$groupId'),
                    onWillAcceptWithDetails: (details) =>
                        details.data.folder.accountId == groupId &&
                        favoriteFolderIds.contains(details.data.folder.id),
                    onAcceptWithDetails: (details) =>
                        onFavoriteFolderOrderChanged(
                          favoriteFolderIds
                              .where((id) => id != details.data.folder.id)
                              .toList(growable: false),
                        ),
                    builder: (context, candidates, _) => Container(
                      height: 31,
                      padding: const EdgeInsets.only(left: 11, right: 3),
                      color: candidates.isEmpty
                          ? Colors.transparent
                          : MaicentaPalette.of(context).dangerTint,
                      child: Row(
                        children: [
                          Expanded(
                            child: InkWell(
                              key: Key('group-toggle-$groupId'),
                              onTap: onToggleFolderCollapsed == null
                                  ? null
                                  : () => onToggleFolderCollapsed!(
                                      'group.$groupId',
                                    ),
                              child: Row(
                                children: [
                                  Icon(
                                    candidates.isNotEmpty
                                        ? Icons.remove_circle_outline
                                        : accountCollapsed(groupId)
                                        ? Icons.keyboard_arrow_right
                                        : Icons.keyboard_arrow_down,
                                    size: 16,
                                  ),
                                  const SizedBox(width: 2),
                                  Expanded(
                                    child: Text(
                                      accountLabels[groupId] ?? groupId,
                                      overflow: TextOverflow.ellipsis,
                                      style: const TextStyle(
                                        fontSize: 12,
                                        fontWeight: FontWeight.w600,
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                          SizedBox.square(
                            dimension: 29,
                            child: PopupMenuButton<String>(
                              key: Key('account-menu-$groupId'),
                              tooltip: localizations.accountMenu,
                              padding: EdgeInsets.zero,
                              icon: const Icon(Icons.more_horiz, size: 16),
                              onSelected: (value) {
                                if (value == 'settings') onAccountSettings();
                                if (value == 'folder') onNewFolder();
                              },
                              itemBuilder: (_) => [
                                PopupMenuItem(
                                  value: 'settings',
                                  child: Text(localizations.accountSettings),
                                ),
                                if (groupId == 'personal')
                                  PopupMenuItem(
                                    value: 'folder',
                                    child: Text(localizations.newLocalFolder),
                                  ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  if (!accountCollapsed(groupId))
                    ..._accountFolderTiles(context, groupId),
                ],
              ],
            ),
          ),
          Container(
            height: 25,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            color: MaicentaPalette.of(context).chrome,
            child: Row(
              children: [
                Icon(
                  accounts.isEmpty
                      ? Icons.cloud_off_outlined
                      : Icons.cloud_done_outlined,
                  size: 13,
                  color: MaicentaPalette.of(context).mutedText,
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    connectionLabel,
                    style: TextStyle(
                      fontSize: 10,
                      color: MaicentaPalette.of(context).mutedText,
                    ),
                  ),
                ),
              ],
            ),
          ),
          ModuleRail(
            selected: module,
            onSelected: onModuleSelected,
            onMoreApps: onMoreApps,
          ),
        ],
      ),
    );
  }

  Widget _favoriteFolderTile(
    BuildContext context, {
    required MailFolder folder,
    String? secondaryLabel,
  }) {
    return DragTarget<FolderDragData>(
      key: Key('favorite-drop-before-${folder.id}'),
      onWillAcceptWithDetails: (details) => details.data.folder.id != folder.id,
      onAcceptWithDetails: (details) {
        final reordered = favoriteFolderIds
            .where((id) => id != details.data.folder.id)
            .toList();
        final targetIndex = reordered.indexOf(folder.id);
        reordered.insert(
          targetIndex < 0 ? reordered.length : targetIndex,
          details.data.folder.id,
        );
        onFavoriteFolderOrderChanged(reordered);
      },
      builder: (context, candidates, _) => DecoratedBox(
        decoration: BoxDecoration(
          border: Border(
            top: BorderSide(
              color: candidates.isEmpty
                  ? Colors.transparent
                  : MaicentaApp.primaryBlue,
              width: 2,
            ),
          ),
        ),
        child: _draggableFolderTile(
          context,
          folder: folder,
          keyPrefix: 'favorite-folder',
          secondaryLabel: secondaryLabel,
          inFavorites: true,
        ),
      ),
    );
  }

  /// Rows of one account as a collapsible tree whose siblings the user can
  /// rearrange by dropping one folder onto another.
  List<Widget> _accountFolderTiles(BuildContext context, String groupId) {
    final tree = buildFolderTree(
      folders
          .where((folder) => folder.accountId == groupId)
          .toList(growable: false),
      orderIds: folderOrderIds,
    );
    final parents = folderTreeParentIds(tree);
    final rowIndex = <String, int>{
      for (var index = 0; index < tree.length; index += 1)
        tree[index].folder.id: index,
    };
    return [
      for (final entry in visibleFolderTree(tree, collapsedFolderIds))
        _reorderTarget(
          tree: tree,
          rowIndex: rowIndex,
          entry: entry,
          child: _draggableFolderTile(
            context,
            folder: entry.folder,
            depth: entry.depth,
            tooltip: entry.folder.role == 'custom' ? entry.path : null,
            expandable: parents.contains(entry.folder.id),
            collapsed: collapsedFolderIds.contains(entry.folder.id),
          ),
        ),
    ];
  }

  /// Drop zone that moves a sibling folder above or below [entry].
  ///
  /// A bar at the top or bottom edge previews where the folder will land;
  /// drops from the favorites list or from another level are refused.
  Widget _reorderTarget({
    required List<FolderTreeEntry> tree,
    required Map<String, int> rowIndex,
    required FolderTreeEntry entry,
    required Widget child,
  }) {
    List<String>? orderFor(FolderDragData data) {
      if (data.fromFavorites || onFolderOrderChanged == null) return null;
      return folderOrderAfterMove(
        tree: tree,
        orderIds: folderOrderIds,
        draggedId: data.folder.id,
        targetId: entry.folder.id,
      );
    }

    return DragTarget<FolderDragData>(
      key: Key('folder-reorder-${entry.folder.id}'),
      onWillAcceptWithDetails: (details) => orderFor(details.data) != null,
      onAcceptWithDetails: (details) {
        final reordered = orderFor(details.data);
        if (reordered != null) onFolderOrderChanged!(reordered);
      },
      builder: (context, candidates, _) {
        final dragged = candidates.firstOrNull;
        if (dragged == null) return child;
        final movesDown =
            (rowIndex[dragged.folder.id] ?? -1) <
            (rowIndex[entry.folder.id] ?? 0);
        final indicator = BorderSide(color: MaicentaApp.primaryBlue, width: 2);
        return Container(
          foregroundDecoration: BoxDecoration(
            border: Border(
              top: movesDown ? BorderSide.none : indicator,
              bottom: movesDown ? indicator : BorderSide.none,
            ),
          ),
          child: child,
        );
      },
    );
  }

  Widget _draggableFolderTile(
    BuildContext context, {
    required MailFolder folder,
    String keyPrefix = 'folder',
    String? secondaryLabel,
    int depth = 0,
    String? tooltip,
    bool? expandable,
    bool collapsed = false,
    bool inFavorites = false,
  }) {
    final label = mailboxDisplayName(context, folder);
    final tile = DragTarget<DemoMessage>(
      key: Key('message-drop-${folder.id}-$keyPrefix'),
      onWillAcceptWithDetails: (details) =>
          details.data.accountId == folder.accountId &&
          details.data.mailboxId != folder.id,
      onAcceptWithDetails: (details) => onMessageDropped(details.data, folder),
      builder: (context, candidates, _) => DecoratedBox(
        decoration: BoxDecoration(
          color: candidates.isEmpty
              ? Colors.transparent
              : MaicentaPalette.of(context).selected,
          border: Border.all(
            color: candidates.isEmpty
                ? Colors.transparent
                : MaicentaApp.primaryBlue,
          ),
        ),
        child: FolderTile(
          id: folder.id,
          keyPrefix: keyPrefix,
          label: label,
          secondaryLabel: secondaryLabel,
          tooltip: tooltip,
          depth: depth,
          expandable: expandable,
          collapsed: collapsed,
          onToggleCollapsed: onToggleFolderCollapsed,
          icon: folderIcon(folder.role),
          count: _folderBadgeCount(folder),
          selected: selectedFolder == folder.id,
          onTap: onSelected,
        ),
      ),
    );
    return Draggable<FolderDragData>(
      data: FolderDragData(folder, fromFavorites: inFavorites),
      dragAnchorStrategy: pointerDragAnchorStrategy,
      rootOverlay: true,
      feedback: _FolderDragFeedback(
        label: label,
        icon: folderIcon(folder.role),
      ),
      childWhenDragging: Opacity(opacity: 0.45, child: tile),
      child: tile,
    );
  }

  int? _folderBadgeCount(MailFolder folder) {
    final count = folder.role == 'drafts'
        ? folder.totalCount
        : folder.unreadCount;
    return count == 0 ? null : count;
  }

  String _accountQualifier(
    MailFolder folder,
    Map<String, String> accountLabels,
  ) {
    final account = accounts
        .where((account) => account.id == folder.accountId)
        .firstOrNull;
    if (account == null) {
      return accountLabels[folder.accountId] ?? folder.accountId;
    }
    final email = account.email.trim();
    return email.isNotEmpty ? email : account.displayName;
  }
}

class _FolderGroupLabel extends StatelessWidget {
  const _FolderGroupLabel({
    required this.label,
    this.trailing,
    this.collapsed = false,
    this.onToggle,
    this.toggleKey,
  });

  final String label;
  final IconData? trailing;
  final bool collapsed;
  final VoidCallback? onToggle;
  final Key? toggleKey;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      key: toggleKey,
      onTap: onToggle,
      child: Container(
        height: 29,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        alignment: Alignment.centerLeft,
        child: Row(
          children: [
            Icon(
              collapsed
                  ? Icons.keyboard_arrow_right
                  : Icons.keyboard_arrow_down,
              size: 16,
            ),
            const SizedBox(width: 2),
            Text(
              label,
              style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
            ),
            const Spacer(),
            if (trailing != null) Icon(trailing, size: 16),
          ],
        ),
      ),
    );
  }
}

/// Progress dialog shown while an account is removed from the profile.
class AccountRemovalDialog extends StatelessWidget {
  const AccountRemovalDialog({
    super.key,
    required this.accountName,
    required this.waitingForSync,
    required this.onKeepAccount,
  });

  final String accountName;

  /// `true` while a synchronization pass has to finish before the removal
  /// can start; the user may still keep the account during that time.
  final ValueListenable<bool> waitingForSync;
  final VoidCallback onKeepAccount;

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<bool>(
      valueListenable: waitingForSync,
      builder: (context, waiting, _) => AlertDialog(
        key: const Key('account-removal-progress'),
        title: const Text('Konto entfernen'),
        content: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
            const SizedBox(width: 16),
            Flexible(
              child: Text(
                waiting
                    ? 'Die laufende Synchronisierung wird zuerst abgeschlossen, '
                          'damit keine Daten dieses Kontos mehr geschrieben werden. '
                          '„$accountName“ wird danach entfernt.'
                    : '„$accountName“ und die lokal zwischengespeicherten '
                          'Nachrichten werden entfernt …',
              ),
            ),
          ],
        ),
        actions: [
          if (waiting)
            TextButton(
              key: const Key('account-removal-keep'),
              onPressed: onKeepAccount,
              child: const Text('Konto behalten'),
            ),
        ],
      ),
    );
  }
}

class _FolderDragFeedback extends StatelessWidget {
  const _FolderDragFeedback({required this.label, required this.icon});

  final String label;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: MaicentaPalette.of(context).pane,
      elevation: 6,
      child: Container(
        width: 210,
        height: 34,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        decoration: BoxDecoration(
          border: Border.all(color: MaicentaApp.primaryBlue),
        ),
        child: Row(
          children: [
            Icon(icon, size: 16, color: MaicentaApp.primaryBlue),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                label,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 12),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class FolderTile extends StatelessWidget {
  const FolderTile({
    super.key,
    required this.id,
    required this.label,
    required this.icon,
    required this.selected,
    required this.onTap,
    this.count,
    this.secondaryLabel,
    this.tooltip,
    this.depth = 0,
    this.expandable,
    this.collapsed = false,
    this.onToggleCollapsed,
    this.keyPrefix = 'folder',
  });

  final String id;
  final String label;
  final IconData icon;
  final bool selected;
  final ValueChanged<String> onTap;
  final int? count;
  final String? secondaryLabel;

  /// Full folder path for nested custom folders; defaults to the label.
  final String? tooltip;

  /// Nesting level; each level indents the row like a classic folder tree.
  final int depth;

  /// `true` shows a collapse toggle, `false` reserves its space so sibling
  /// rows stay aligned, `null` renders a plain row without the gutter.
  final bool? expandable;
  final bool collapsed;
  final ValueChanged<String>? onToggleCollapsed;
  final String keyPrefix;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      key: Key('$keyPrefix-$id'),
      onTap: () => onTap(id),
      child: Container(
        height: 27,
        decoration: BoxDecoration(
          color: selected
              ? MaicentaPalette.of(context).selected
              : Colors.transparent,
          border: Border(
            left: BorderSide(
              color: selected ? MaicentaApp.primaryBlue : Colors.transparent,
              width: 2,
            ),
          ),
        ),
        padding: EdgeInsets.only(
          left: (expandable == null ? 27.0 : 9.0) + depth * 14,
          right: 12,
        ),
        child: Row(
          children: [
            if (expandable != null)
              SizedBox(
                width: 18,
                height: 27,
                child: expandable!
                    ? InkWell(
                        key: Key('folder-toggle-$id'),
                        onTap: onToggleCollapsed == null
                            ? null
                            : () => onToggleCollapsed!(id),
                        child: Tooltip(
                          message: collapsed
                              ? 'Unterordner anzeigen'
                              : 'Unterordner ausblenden',
                          child: Icon(
                            collapsed ? Icons.chevron_right : Icons.expand_more,
                            size: 15,
                          ),
                        ),
                      )
                    : null,
              ),
            Icon(
              icon,
              size: 15,
              color: selected ? MaicentaApp.primaryBlue : null,
            ),
            const SizedBox(width: 7),
            Expanded(
              child: Tooltip(
                message: secondaryLabel == null
                    ? (tooltip ?? label)
                    : '${tooltip ?? label} — $secondaryLabel',
                child: Text.rich(
                  TextSpan(
                    text: label,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: selected
                          ? FontWeight.w600
                          : FontWeight.normal,
                    ),
                    children: [
                      if (secondaryLabel != null)
                        TextSpan(
                          text: '  ($secondaryLabel)',
                          style: TextStyle(
                            fontSize: 10,
                            fontWeight: FontWeight.normal,
                            color: MaicentaPalette.of(context).mutedText,
                          ),
                        ),
                    ],
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
            if (count != null)
              Text(
                '$count',
                style: TextStyle(
                  fontSize: 11,
                  color: selected
                      ? MaicentaApp.primaryBlue
                      : MaicentaPalette.of(context).mutedText,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class MessageList extends StatelessWidget {
  const MessageList({
    super.key,
    required this.messages,
    required this.folders,
    required this.selectedIndex,
    required this.folder,
    required this.totalMessageCount,
    required this.hasMore,
    required this.loadingMore,
    required this.onLoadMore,
    required this.onSelected,
    required this.onContextSelected,
    required this.onOpened,
    required this.onContextAction,
    required this.onMoved,
    required this.filter,
    required this.onFilterChanged,
  });

  final List<DemoMessage> messages;
  final List<MailFolder> folders;
  final int selectedIndex;
  final String folder;
  final int totalMessageCount;
  final bool hasMore;
  final bool loadingMore;
  final VoidCallback onLoadMore;
  final ValueChanged<int> onSelected;
  final ValueChanged<int> onContextSelected;
  final ValueChanged<int> onOpened;
  final Future<void> Function(DemoMessage message, MailContextAction action)
  onContextAction;
  final void Function(DemoMessage message, MailFolder folder) onMoved;
  final MailListFilter filter;
  final ValueChanged<MailListFilter> onFilterChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 39,
          padding: const EdgeInsets.only(left: 13, right: 5),
          decoration: BoxDecoration(
            border: Border(
              bottom: BorderSide(color: MaicentaPalette.of(context).border),
            ),
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  folder,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 15,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Text(
                totalMessageCount == messages.length
                    ? '${messages.length} Elemente'
                    : '${messages.length} von $totalMessageCount Elementen',
                style: TextStyle(
                  fontSize: 10,
                  color: MaicentaPalette.of(context).mutedText,
                ),
              ),
            ],
          ),
        ),
        Container(
          height: 31,
          padding: const EdgeInsets.only(left: 13, right: 4),
          decoration: BoxDecoration(
            color: MaicentaPalette.of(context).pane,
            border: Border(
              bottom: BorderSide(color: MaicentaPalette.of(context).border),
            ),
          ),
          child: Row(
            children: [
              _MessageFilterTab(
                label: 'Alle',
                selected: filter == MailListFilter.all,
                onTap: () => onFilterChanged(MailListFilter.all),
              ),
              _MessageFilterTab(
                label: 'Ungelesen',
                selected: filter == MailListFilter.unread,
                onTap: () => onFilterChanged(MailListFilter.unread),
              ),
              const Spacer(),
              Text(
                'Nach Datum',
                style: TextStyle(
                  fontSize: 10,
                  color: MaicentaPalette.of(context).mutedText,
                ),
              ),
              PopupMenuButton<MailListFilter>(
                key: const Key('mail-filter'),
                tooltip: 'Sortieren und filtern',
                padding: EdgeInsets.zero,
                icon: const Icon(Icons.keyboard_arrow_down, size: 17),
                initialValue: filter,
                onSelected: onFilterChanged,
                itemBuilder: (_) => const [
                  PopupMenuItem(
                    value: MailListFilter.all,
                    child: Text('Alle Nachrichten'),
                  ),
                  PopupMenuItem(
                    value: MailListFilter.unread,
                    child: Text('Nur ungelesen'),
                  ),
                  PopupMenuItem(
                    value: MailListFilter.flagged,
                    child: Text('Nur markiert'),
                  ),
                ],
              ),
            ],
          ),
        ),
        if (messages.isEmpty)
          Expanded(
            child: Center(
              child: Text(
                'Keine Nachrichten gefunden',
                style: TextStyle(color: MaicentaPalette.of(context).mutedText),
              ),
            ),
          )
        else
          Expanded(
            child: ListView.builder(
              itemCount: messages.length + (hasMore ? 1 : 0),
              itemBuilder: (context, index) {
                if (index == messages.length) {
                  return SizedBox(
                    height: 54,
                    child: Center(
                      child: TextButton.icon(
                        key: const Key('load-more-messages'),
                        onPressed: loadingMore ? null : onLoadMore,
                        icon: loadingMore
                            ? const SizedBox.square(
                                dimension: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              )
                            : const Icon(Icons.expand_more),
                        label: Text(
                          loadingMore
                              ? 'Ältere Nachrichten werden geladen …'
                              : 'Ältere Nachrichten laden',
                        ),
                      ),
                    ),
                  );
                }
                final message = messages[index];
                return MessageTile(
                  message: message,
                  folders: folders,
                  selected: index == selectedIndex,
                  onTap: () => onSelected(index),
                  onContextSelect: () => onContextSelected(index),
                  onDoubleTap: () => onOpened(index),
                  onContextAction: (action) => onContextAction(message, action),
                  onMoved: (folder) => onMoved(message, folder),
                );
              },
            ),
          ),
      ],
    );
  }
}

class _MessageFilterTab extends StatelessWidget {
  const _MessageFilterTab({
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      child: Container(
        height: 31,
        margin: const EdgeInsets.only(right: 15),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          border: Border(
            bottom: BorderSide(
              color: selected ? MaicentaApp.primaryBlue : Colors.transparent,
              width: 2,
            ),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 11,
            color: selected
                ? Theme.of(context).colorScheme.primary
                : Theme.of(context).colorScheme.onSurface,
            fontWeight: selected ? FontWeight.w600 : FontWeight.normal,
          ),
        ),
      ),
    );
  }
}

class MessageTile extends StatefulWidget {
  const MessageTile({
    super.key,
    required this.message,
    required this.folders,
    required this.selected,
    required this.onTap,
    required this.onContextSelect,
    required this.onDoubleTap,
    required this.onContextAction,
    required this.onMoved,
  });

  final DemoMessage message;
  final List<MailFolder> folders;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback onContextSelect;
  final VoidCallback onDoubleTap;
  final ValueChanged<MailContextAction> onContextAction;
  final ValueChanged<MailFolder> onMoved;

  @override
  State<MessageTile> createState() => _MessageTileState();
}

class _MessageTileState extends State<MessageTile> {
  DateTime? lastTapAt;

  MailFolder? get currentFolder => widget.folders
      .where((folder) => folder.id == widget.message.mailboxId)
      .firstOrNull;

  MailFolder? folderForRole(String role) => widget.folders
      .where(
        (folder) =>
            folder.accountId == widget.message.accountId && folder.role == role,
      )
      .firstOrNull;

  List<MailFolder> get moveDestinations => widget.folders
      .where(
        (folder) =>
            folder.accountId == widget.message.accountId &&
            folder.id != widget.message.mailboxId,
      )
      .toList(growable: false);

  void handleTap() {
    final now = DateTime.now();
    final previous = lastTapAt;
    lastTapAt = now;
    widget.onTap();
    if (previous != null &&
        now.difference(previous) < const Duration(milliseconds: 450)) {
      lastTapAt = null;
      widget.onDoubleTap();
    }
  }

  RelativeRect menuPosition(TapDownDetails details) {
    final overlay =
        Overlay.of(context).context.findRenderObject()! as RenderBox;
    return RelativeRect.fromRect(
      Rect.fromPoints(details.globalPosition, details.globalPosition),
      Offset.zero & overlay.size,
    );
  }

  Future<void> showContextMenu(TapDownDetails details) async {
    widget.onContextSelect();
    final currentRole = currentFolder?.role;
    final archive = folderForRole('archive');
    final trash = folderForRole('trash');
    final junk = folderForRole('junk');
    final destinations = moveDestinations;
    final position = menuPosition(details);
    final action = await showMenu<MailContextAction>(
      context: context,
      position: position,
      items: [
        _mailContextItem(
          action: MailContextAction.open,
          icon: widget.message.draft
              ? Icons.edit_outlined
              : Icons.open_in_new_outlined,
          label: widget.message.draft ? 'Entwurf bearbeiten' : 'Öffnen',
        ),
        if (!widget.message.draft) ...[
          const PopupMenuDivider(),
          _mailContextItem(
            action: MailContextAction.reply,
            icon: Icons.reply,
            label: 'Antworten',
          ),
          _mailContextItem(
            action: MailContextAction.replyAll,
            icon: Icons.reply_all,
            label: 'Allen antworten',
          ),
          _mailContextItem(
            action: MailContextAction.forward,
            icon: Icons.forward,
            label: 'Weiterleiten',
          ),
        ],
        const PopupMenuDivider(),
        if (!widget.message.draft)
          _mailContextItem(
            action: MailContextAction.toggleRead,
            icon: widget.message.unread
                ? Icons.mark_email_read_outlined
                : Icons.mark_email_unread_outlined,
            label: widget.message.unread
                ? 'Als gelesen markieren'
                : 'Als ungelesen markieren',
          ),
        _mailContextItem(
          action: MailContextAction.toggleFlag,
          icon: widget.message.flagged ? Icons.flag : Icons.outlined_flag,
          label: widget.message.flagged
              ? 'Nachverfolgung löschen'
              : 'Zur Nachverfolgung',
        ),
        if (archive != null && currentRole != 'archive')
          _mailContextItem(
            action: MailContextAction.archive,
            icon: Icons.archive_outlined,
            label: 'Archivieren',
          ),
        if (destinations.isNotEmpty)
          _mailContextItem(
            action: MailContextAction.move,
            icon: Icons.drive_file_move_outlined,
            label: 'Verschieben',
            trailing: Icons.chevron_right,
          ),
        if (!widget.message.draft && currentRole == 'junk')
          _mailContextItem(
            action: MailContextAction.notSpam,
            icon: Icons.mark_email_read_outlined,
            label: 'Kein Spam',
            enabled: folderForRole('inbox') != null,
          )
        else if (!widget.message.draft && junk != null)
          _mailContextItem(
            action: MailContextAction.spam,
            icon: Icons.report_gmailerrorred_outlined,
            label: 'Als Spam behandeln',
          ),
        if (trash != null && currentRole != 'trash') ...[
          const PopupMenuDivider(),
          _mailContextItem(
            action: MailContextAction.delete,
            icon: Icons.delete_outline,
            label: 'Löschen',
            destructive: true,
          ),
        ],
      ],
    );
    if (!mounted || action == null) return;
    if (action != MailContextAction.move) {
      widget.onContextAction(action);
      return;
    }
    await Future<void>.delayed(Duration.zero);
    if (!mounted) return;
    final mailboxId = await showMenu<String>(
      context: context,
      position: position,
      items: [
        for (final folder in destinations)
          PopupMenuItem<String>(
            key: Key('mail-context-move-${folder.id}'),
            value: folder.id,
            child: Row(
              children: [
                Icon(folderIcon(folder.role), size: 18),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    mailboxDisplayName(context, folder),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
      ],
    );
    if (!mounted || mailboxId == null) return;
    final destination = destinations
        .where((folder) => folder.id == mailboxId)
        .firstOrNull;
    if (destination != null) widget.onMoved(destination);
  }

  @override
  Widget build(BuildContext context) {
    final tile = InkWell(
      onTap: handleTap,
      onSecondaryTapDown: showContextMenu,
      child: Container(
        height: 70,
        padding: const EdgeInsets.fromLTRB(11, 7, 9, 6),
        decoration: BoxDecoration(
          color: widget.selected
              ? MaicentaPalette.of(context).selectedStrong
              : widget.message.unread
              ? MaicentaPalette.of(context).unread
              : MaicentaPalette.of(context).window,
          border: Border(
            bottom: BorderSide(color: MaicentaPalette.of(context).border),
          ),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              width: 5,
              height: 5,
              margin: const EdgeInsets.only(top: 6, right: 7),
              decoration: BoxDecoration(
                color: widget.message.unread
                    ? MaicentaApp.primaryBlue
                    : Colors.transparent,
                shape: BoxShape.circle,
              ),
            ),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          widget.message.sender,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 12.5,
                            fontWeight: widget.message.unread
                                ? FontWeight.w700
                                : FontWeight.w500,
                          ),
                        ),
                      ),
                      Text(
                        widget.message.time,
                        style: TextStyle(
                          fontSize: 9.5,
                          color: MaicentaPalette.of(context).mutedText,
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 2),
                  Text(
                    widget.message.subject,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 11,
                      fontWeight: widget.message.unread
                          ? FontWeight.w600
                          : FontWeight.normal,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          widget.message.preview,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 10,
                            color: MaicentaPalette.of(context).mutedText,
                          ),
                        ),
                      ),
                      if (widget.message.hasAttachment)
                        const Padding(
                          padding: EdgeInsets.only(left: 5),
                          child: Icon(Icons.attach_file, size: 14),
                        ),
                      if (widget.message.flagged)
                        const Padding(
                          padding: EdgeInsets.only(left: 5),
                          child: Icon(
                            Icons.flag,
                            size: 14,
                            color: Color(0xFFC74432),
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
    return Draggable<DemoMessage>(
      data: widget.message,
      dragAnchorStrategy: pointerDragAnchorStrategy,
      rootOverlay: true,
      feedback: _MessageDragFeedback(message: widget.message),
      childWhenDragging: Opacity(opacity: 0.45, child: tile),
      child: tile,
    );
  }
}

PopupMenuItem<MailContextAction> _mailContextItem({
  required MailContextAction action,
  required IconData icon,
  required String label,
  IconData? trailing,
  bool enabled = true,
  bool destructive = false,
}) {
  final color = !enabled
      ? const Color(0xFF999999)
      : destructive
      ? const Color(0xFFC42B1C)
      : null;
  return PopupMenuItem<MailContextAction>(
    key: Key('mail-context-${action.name}'),
    value: action,
    enabled: enabled,
    height: 38,
    child: Row(
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 11),
        Expanded(
          child: Text(label, style: TextStyle(fontSize: 12, color: color)),
        ),
        if (trailing != null) Icon(trailing, size: 17, color: color),
      ],
    ),
  );
}

class _MessageDragFeedback extends StatelessWidget {
  const _MessageDragFeedback({required this.message});

  final DemoMessage message;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: MaicentaPalette.of(context).pane,
      elevation: 8,
      child: Container(
        width: 300,
        padding: const EdgeInsets.symmetric(horizontal: 13, vertical: 10),
        decoration: BoxDecoration(
          border: Border.all(color: MaicentaApp.primaryBlue),
        ),
        child: Row(
          children: [
            const Icon(
              Icons.mail_outline,
              size: 18,
              color: MaicentaApp.primaryBlue,
            ),
            const SizedBox(width: 9),
            Expanded(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    message.sender,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 11,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  Text(
                    message.subject,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 11),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class ReadingPane extends StatefulWidget {
  const ReadingPane({
    super.key,
    required this.message,
    required this.onReply,
    required this.onForward,
    required this.onEditDraft,
    required this.onSaveAttachment,
    this.onReloadContent,
    required this.zoom,
  });

  final DemoMessage? message;
  final VoidCallback onReply;
  final VoidCallback onForward;
  final VoidCallback onEditDraft;
  final ValueChanged<MailAttachmentData> onSaveAttachment;
  final Future<DemoMessage?> Function(DemoMessage message)? onReloadContent;
  final double zoom;

  @override
  State<ReadingPane> createState() => _ReadingPaneState();
}

class _ReadingPaneState extends State<ReadingPane> {
  bool externalImagesAllowed = false;
  bool loadingRemoteContent = false;

  @override
  void didUpdateWidget(covariant ReadingPane oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.message?.id != widget.message?.id) {
      externalImagesAllowed = false;
      loadingRemoteContent = false;
    }
  }

  Future<void> showExternalContent(DemoMessage current) async {
    if (hasBlockedRemoteImages(current.body)) {
      setState(() => externalImagesAllowed = true);
      return;
    }

    final reload = widget.onReloadContent;
    if (reload == null || loadingRemoteContent) return;
    setState(() => loadingRemoteContent = true);
    final refreshed = await reload(current);
    if (!mounted) return;
    setState(() {
      loadingRemoteContent = false;
      externalImagesAllowed =
          refreshed != null && hasBlockedRemoteImages(refreshed.body);
    });
    if (refreshed != null && !externalImagesAllowed) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text(
            'Die Serverkopie enthält keine darstellbaren externen Bilder.',
          ),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final message = widget.message;
    final onReply = widget.onReply;
    final onForward = widget.onForward;
    final onEditDraft = widget.onEditDraft;
    final onSaveAttachment = widget.onSaveAttachment;
    final zoom = widget.zoom;
    if (message == null) {
      return const Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.search_off, size: 42, color: Color(0xFFAAAAAA)),
            SizedBox(height: 12),
            Text('Keine Nachricht ausgewählt'),
          ],
        ),
      );
    }

    final current = message;
    final blockedRemoteImages = hasBlockedRemoteImages(current.body);
    final unresolvedImages = hasUnresolvedMessageImages(current.body);
    final hasVisibleText = hasDisplayableMessageText(
      current.body,
      current.plainText,
    );
    final canRequestRemoteContent =
        !current.draft &&
        current.accountId != 'personal' &&
        (blockedRemoteImages || unresolvedImages || !hasVisibleText);
    return Container(
      key: const Key('reading-pane'),
      color: MaicentaPalette.of(context).window,
      child: SingleChildScrollView(
        key: const Key('reading-pane-scroll'),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 16, 16, 11),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    current.subject,
                    style: TextStyle(
                      fontSize: 19 * zoom,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const SizedBox(height: 13),
                  _ReadingMessageHeader(
                    message: current,
                    onReply: onReply,
                    onForward: onForward,
                    onEditDraft: onEditDraft,
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Container(
              color: MaicentaPalette.of(context).chrome,
              padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 5),
              constraints: const BoxConstraints(minHeight: 27),
              child: Row(
                children: [
                  Icon(
                    current.draft
                        ? Icons.edit_note_outlined
                        : Icons.shield_outlined,
                    size: 16,
                    color: MaicentaApp.primaryBlue,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      current.draft
                          ? current.editableDraft
                                ? current.draftSynchronized
                                      ? 'IMAP-Entwurf · Lokal bearbeitbar und synchronisiert'
                                      : 'Lokaler Entwurf · IMAP-Synchronisierung ausstehend'
                                : 'Serverentwurf · Remote-Bestandteile schützen die Nachricht vor unvollständiger Bearbeitung'
                          : externalImagesAllowed
                          ? 'Sichere HTML-Ansicht · Externe Bilder für diese Nachricht geladen · Skripte blockiert'
                          : blockedRemoteImages
                          ? 'Sichere HTML-Ansicht · Externe Bilder und Skripte sind blockiert'
                          : 'Sichere HTML-Ansicht · Skripte und externe Inhalte sind blockiert',
                      style: const TextStyle(fontSize: 11),
                    ),
                  ),
                  if (canRequestRemoteContent) ...[
                    const SizedBox(width: 8),
                    TextButton.icon(
                      key: const Key('reading-load-external-content'),
                      onPressed: loadingRemoteContent
                          ? null
                          : () => showExternalContent(current),
                      icon: loadingRemoteContent
                          ? const SizedBox.square(
                              dimension: 14,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.image_outlined, size: 16),
                      label: Text(
                        blockedRemoteImages || unresolvedImages
                            ? 'Bilder laden'
                            : 'Inhalt erneut laden',
                        style: const TextStyle(fontSize: 11),
                      ),
                    ),
                  ],
                ],
              ),
            ),
            if (!hasVisibleText && !externalImagesAllowed)
              Padding(
                padding: const EdgeInsets.fromLTRB(22, 18, 28, 0),
                child: Text(
                  blockedRemoteImages
                      ? 'Diese Nachricht enthält keinen lokal darstellbaren Text. Sie besteht möglicherweise nur aus externen Bildern.'
                      : 'Für diese Nachricht ist kein darstellbarer Text lokal gespeichert. Der Inhalt kann erneut vom Mailserver geladen werden.',
                  style: TextStyle(
                    fontSize: 12,
                    color: MaicentaPalette.of(context).mutedText,
                  ),
                ),
              ),
            Padding(
              padding: const EdgeInsets.fromLTRB(22, 22, 28, 28),
              child: SelectionArea(
                child: HtmlWidget(
                  key: const Key('sanitized-html-body'),
                  current.body,
                  factoryBuilder: () => SafeMailWidgetFactory(
                    allowRemoteImages: () => externalImagesAllowed,
                  ),
                  rebuildTriggers: [externalImagesAllowed, current.body],
                  textStyle: TextStyle(fontSize: 13 * zoom, height: 1.48),
                  onTapUrl: (url) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(
                        content: Text(
                          'Externer Link wurde nicht automatisch geöffnet: $url',
                        ),
                      ),
                    );
                    return true;
                  },
                ),
              ),
            ),
            if (current.attachments.isNotEmpty)
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: MaicentaPalette.of(context).pane,
                  border: Border(
                    top: BorderSide(color: MaicentaPalette.of(context).border),
                  ),
                ),
                child: LayoutBuilder(
                  builder: (context, constraints) {
                    final tileWidth = constraints.maxWidth > 360
                        ? 360.0
                        : constraints.maxWidth;
                    return Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      children: [
                        for (final attachment in current.attachments)
                          SizedBox(
                            width: tileWidth,
                            child: Material(
                              color: MaicentaPalette.of(context).window,
                              shape: RoundedRectangleBorder(
                                side: BorderSide(
                                  color: MaicentaPalette.of(context).border,
                                ),
                                borderRadius: BorderRadius.circular(3),
                              ),
                              child: InkWell(
                                key: Key('message-attachment-${attachment.id}'),
                                onTap: () => onSaveAttachment(attachment),
                                child: Padding(
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: 12,
                                    vertical: 9,
                                  ),
                                  child: Row(
                                    children: [
                                      const Icon(
                                        Icons.insert_drive_file_outlined,
                                        size: 20,
                                        color: MaicentaApp.primaryBlue,
                                      ),
                                      const SizedBox(width: 8),
                                      Expanded(
                                        child: Column(
                                          crossAxisAlignment:
                                              CrossAxisAlignment.start,
                                          children: [
                                            Text(
                                              attachment.fileName,
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                              style: const TextStyle(
                                                fontWeight: FontWeight.w500,
                                              ),
                                            ),
                                            Text(
                                              '${attachment.contentType} · ${formatFileSize(attachment.sizeBytes)}${attachment.availableLocally ? '' : ' · Auf Server'}',
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                              style: TextStyle(
                                                fontSize: 10,
                                                color: MaicentaPalette.of(
                                                  context,
                                                ).mutedText,
                                              ),
                                            ),
                                          ],
                                        ),
                                      ),
                                      const SizedBox(width: 8),
                                      Tooltip(
                                        message: attachment.availableLocally
                                            ? 'Speichern unter …'
                                            : 'Vom Server laden und speichern …',
                                        child: Icon(
                                          attachment.availableLocally
                                              ? Icons.download_outlined
                                              : Icons.cloud_download_outlined,
                                          size: 19,
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                            ),
                          ),
                      ],
                    );
                  },
                ),
              )
            else if (current.hasAttachment)
              Container(
                key: const Key('remote-attachment-pending'),
                padding: const EdgeInsets.all(14),
                decoration: BoxDecoration(
                  color: MaicentaPalette.of(context).pane,
                  border: Border(
                    top: BorderSide(color: MaicentaPalette.of(context).border),
                  ),
                ),
                child: const Row(
                  children: [
                    Icon(Icons.cloud_download_outlined, size: 19),
                    SizedBox(width: 9),
                    Expanded(
                      child: Text(
                        'Anhang ist auf dem Mailserver vorhanden, lokal aber noch nicht verfügbar. Erneut synchronisieren oder Größenlimit prüfen.',
                        style: TextStyle(fontSize: 11),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _ReadingMessageHeader extends StatelessWidget {
  const _ReadingMessageHeader({
    required this.message,
    required this.onReply,
    required this.onForward,
    required this.onEditDraft,
  });

  final DemoMessage message;
  final VoidCallback onReply;
  final VoidCallback onForward;
  final VoidCallback onEditDraft;

  Widget _avatar(BuildContext context) {
    return CircleAvatar(
      radius: 17,
      backgroundColor: MaicentaPalette.of(context).selected,
      child: Text(
        initials(message.sender),
        style: const TextStyle(
          color: MaicentaApp.primaryBlue,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }

  Widget _senderDetails(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          message.sender,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontWeight: FontWeight.w600),
        ),
        Text(
          message.draft
              ? 'An: ${message.draftTo.isEmpty ? "Noch kein Empfänger" : message.draftTo}'
              : '${message.email}  ·  an mich',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            fontSize: 11,
            color: MaicentaPalette.of(context).mutedText,
          ),
        ),
      ],
    );
  }

  Widget _timestamp(BuildContext context) {
    return Text(
      message.time,
      maxLines: 1,
      style: TextStyle(
        fontSize: 11,
        color: MaicentaPalette.of(context).mutedText,
      ),
    );
  }

  Widget _actions() {
    if (message.draft) {
      return FilledButton.icon(
        key: const Key('reading-edit-draft'),
        onPressed: onEditDraft,
        icon: const Icon(Icons.edit_outlined, size: 18),
        label: const Text('Entwurf bearbeiten', maxLines: 1, softWrap: false),
        style: FilledButton.styleFrom(
          padding: const EdgeInsets.symmetric(horizontal: 12),
        ),
      );
    }
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        IconButton(
          key: const Key('reading-reply'),
          tooltip: 'Antworten',
          onPressed: onReply,
          icon: const Icon(Icons.reply, size: 19),
        ),
        PopupMenuButton<String>(
          tooltip: 'Weitere Aktionen',
          icon: const Icon(Icons.more_horiz, size: 19),
          onSelected: (value) {
            if (value == 'forward') onForward();
          },
          itemBuilder: (_) => const [
            PopupMenuItem(value: 'forward', child: Text('Weiterleiten')),
          ],
        ),
      ],
    );
  }

  Widget _identity(BuildContext context) {
    return Row(
      children: [
        _avatar(context),
        const SizedBox(width: 11),
        Expanded(child: _senderDetails(context)),
        const SizedBox(width: 8),
        _timestamp(context),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        if (constraints.maxWidth < 440) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _identity(context),
              const SizedBox(height: 8),
              _actions(),
            ],
          );
        }
        return Row(
          children: [
            _avatar(context),
            const SizedBox(width: 11),
            Expanded(child: _senderDetails(context)),
            const SizedBox(width: 8),
            _timestamp(context),
            const SizedBox(width: 8),
            _actions(),
          ],
        );
      },
    );
  }
}

class StatusBar extends StatelessWidget {
  const StatusBar({
    super.key,
    required this.module,
    this.summary = '',
    required this.itemCount,
    required this.unreadCount,
    required this.pendingMailOperations,
    this.synchronizing = false,
    this.catalogMessagesRemaining = 0,
    required this.offlineMode,
    required this.zoom,
  });

  final WorkspaceModule module;

  /// Text shown for the non-mail modules.
  final String summary;
  final int itemCount;
  final int unreadCount;
  final int pendingMailOperations;

  /// Whether an account synchronization pass is running right now.
  final bool synchronizing;

  /// Catalogue entries the running synchronization still has to fetch.
  final int catalogMessagesRemaining;
  final bool offlineMode;
  final double zoom;

  @override
  Widget build(BuildContext context) {
    final label = switch (module) {
      WorkspaceModule.mail =>
        'Elemente: $itemCount · Ungelesen: $unreadCount'
            '${pendingMailOperations == 0 ? '' : ' · $pendingMailOperations ausstehend'}',
      WorkspaceModule.calendar ||
      WorkspaceModule.tasks ||
      WorkspaceModule.contacts => summary,
    };
    final syncStatus = synchronizing
        ? catalogMessagesRemaining > 0
              ? 'Synchronisierung läuft · noch $catalogMessagesRemaining Nachrichten'
              : 'Synchronisierung läuft …'
        : offlineMode
        ? 'Offline · lokaler Datenbestand'
        : 'Alle Ordner sind verfügbar';
    final primaryStatus = module == WorkspaceModule.mail
        ? '$label   |   $syncStatus'
        : label;
    return Container(
      key: const Key('classic-status-bar'),
      height: 23,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).subtle,
        border: Border(
          top: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Row(
        children: [
          if (module == WorkspaceModule.mail && synchronizing) ...[
            SizedBox.square(
              key: const Key('status-sync-indicator'),
              dimension: 10,
              child: CircularProgressIndicator(
                strokeWidth: 1.5,
                color: MaicentaPalette.of(context).mutedText,
              ),
            ),
            const SizedBox(width: 6),
          ],
          Expanded(
            child: Text(
              primaryStatus,
              key: const Key('status-primary-text'),
              overflow: TextOverflow.ellipsis,
              maxLines: 1,
              style: TextStyle(
                fontSize: 10,
                color: MaicentaPalette.of(context).mutedText,
              ),
            ),
          ),
          if (module == WorkspaceModule.mail)
            Text(
              '${(zoom * 100).round()} %',
              style: TextStyle(
                fontSize: 10,
                color: MaicentaPalette.of(context).mutedText,
              ),
            ),
          const _StatusDivider(),
          Icon(
            Icons.lock_outline,
            size: 12,
            color: MaicentaPalette.of(context).mutedText,
          ),
          const SizedBox(width: 4),
          Text(
            offlineMode ? 'Lokaler Offline-Modus' : 'Lokaler Online-Modus',
            style: TextStyle(
              fontSize: 10,
              color: MaicentaPalette.of(context).mutedText,
            ),
          ),
        ],
      ),
    );
  }
}

class _StatusDivider extends StatelessWidget {
  const _StatusDivider();

  @override
  Widget build(BuildContext context) {
    return const Padding(
      padding: EdgeInsets.symmetric(horizontal: 8),
      child: VerticalDivider(width: 1, indent: 4, endIndent: 4),
    );
  }
}

String initials(String name) {
  final parts = name.trim().split(RegExp(r'\s+'));
  return parts.take(2).map((part) => part[0].toUpperCase()).join();
}

String prefixedSubject(String subject, String prefix) {
  final normalized = subject.trimLeft();
  if (normalized.toLowerCase().startsWith(prefix.toLowerCase())) {
    return normalized;
  }
  return '$prefix $normalized';
}

List<ComposeSender> composeSenders(List<MailAccountConfig> accounts) {
  if (accounts.isEmpty) {
    return const [
      ComposeSender(
        accountId: 'personal',
        address: 'demo@maicenta.local',
        label: 'Persönliches Konto <demo@maicenta.local>',
      ),
    ];
  }
  return [
    for (final account in accounts)
      for (final identity in account.senderIdentities)
        ComposeSender(
          accountId: account.id,
          address: identity.address,
          label: identity.label,
        ),
  ];
}

/// Picks the address a reply should come from: the alias (or primary
/// address) of [account] that the original message was sent to. Returns an
/// empty string when none matches so the composer uses the primary address.
String replySenderAddress(MailAccountConfig? account, DemoMessage message) {
  if (account == null || account.aliases.isEmpty) return '';
  final recipients = [
    ...parseRecipientList(message.toRecipients),
    ...parseRecipientList(message.ccRecipients),
  ].map(recipientAddress);
  for (final recipient in recipients) {
    final identity = account.senderFor(recipient);
    if (identity != null) return identity.address;
  }
  return '';
}

/// Extracts the bare address from `Name <address>` or returns the input.
String recipientAddress(String recipient) {
  final match = RegExp(r'<([^<>]+)>').firstMatch(recipient);
  return (match?.group(1) ?? recipient).trim();
}

List<String> parseRecipientList(String value) {
  return value
      .split(RegExp(r'[,;]'))
      .map((recipient) => recipient.trim())
      .where((recipient) => recipient.isNotEmpty)
      .toList(growable: false);
}

String quotedMessage(DemoMessage message) {
  return '\n\n--- Ursprüngliche Nachricht ---\n'
      'Von: ${message.sender} <${message.email}>\n'
      'Betreff: ${message.subject}\n\n'
      '${message.preview}\n';
}

Future<String?> showTextPrompt(
  BuildContext context, {
  required String title,
  required String label,
  String initialValue = '',
}) async {
  var value = initialValue;
  return showDialog<String>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      title: Text(title),
      content: TextFormField(
        key: const Key('prompt-input'),
        initialValue: initialValue,
        autofocus: true,
        decoration: InputDecoration(labelText: label),
        onChanged: (updated) => value = updated,
        onFieldSubmitted: (submitted) =>
            Navigator.pop(dialogContext, submitted),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(dialogContext),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(dialogContext, value),
          child: const Text('Speichern'),
        ),
      ],
    ),
  );
}

String folderDisplayName(
  BuildContext context,
  List<MailFolder> folders,
  String selectedId,
) {
  final localizations = AppLocalizations.of(context);
  if (selectedId == 'virtual.flagged') return localizations.virtualFlagged;
  if (selectedId == 'virtual.unread') return localizations.virtualUnread;
  for (final folder in folders) {
    if (folder.id == selectedId) return mailboxDisplayName(context, folder);
  }
  return localizations.mailboxInbox;
}

IconData folderIcon(String role) {
  return switch (role) {
    'inbox' => Icons.inbox_outlined,
    'drafts' => Icons.edit_note_outlined,
    'sent' => Icons.send_outlined,
    'archive' => Icons.archive_outlined,
    'trash' => Icons.delete_outline,
    'junk' => Icons.report_gmailerrorred_outlined,
    _ => Icons.folder_outlined,
  };
}
