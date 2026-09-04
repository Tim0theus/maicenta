import '../../src/rust/api/workspace.dart' as rust;
import 'oauth_service.dart';

class MailFolder {
  const MailFolder({
    required this.id,
    this.accountId = 'personal',
    required this.displayName,
    required this.role,
    required this.unreadCount,
    required this.totalCount,
  });

  final String id;
  final String accountId;
  final String displayName;
  final String role;
  final int unreadCount;
  final int totalCount;

  MailFolder copyWith({
    String? displayName,
    int? unreadCount,
    int? totalCount,
  }) {
    return MailFolder(
      id: id,
      accountId: accountId,
      displayName: displayName ?? this.displayName,
      role: role,
      unreadCount: unreadCount ?? this.unreadCount,
      totalCount: totalCount ?? this.totalCount,
    );
  }
}

/// Message data required by the current prototype interface.
///
/// This view model deliberately does not contain provider- or database-specific
/// types. `body` contains HTML sanitized by the Rust core. Plain-text messages
/// are escaped and wrapped as safe HTML before crossing the bridge.
class DemoMessage {
  const DemoMessage({
    required this.id,
    this.accountId = 'personal',
    required this.mailboxId,
    required this.sender,
    required this.email,
    required this.subject,
    required this.preview,
    required this.body,
    this.plainText = '',
    required this.time,
    this.unread = false,
    this.flagged = false,
    this.hasAttachment = false,
    this.attachments = const [],
    this.draft = false,
    this.editableDraft = false,
    this.draftSynchronized = false,
    this.draftTo = '',
    this.draftCc = '',
    this.draftBcc = '',
    this.toRecipients = '',
    this.ccRecipients = '',
    this.bccRecipients = '',
    this.editorDeltaJson = '',
  });

  final String id;
  final String accountId;
  final String mailboxId;
  final String sender;
  final String email;
  final String subject;
  final String preview;
  final String body;
  final String plainText;
  final String time;
  final bool unread;
  final bool flagged;
  final bool hasAttachment;
  final List<MailAttachmentData> attachments;
  final bool draft;
  final bool editableDraft;
  final bool draftSynchronized;
  final String draftTo;
  final String draftCc;
  final String draftBcc;
  final String toRecipients;
  final String ccRecipients;
  final String bccRecipients;
  final String editorDeltaJson;

  DemoMessage copyWith({
    String? mailboxId,
    bool? unread,
    bool? flagged,
    bool? draftSynchronized,
  }) {
    return DemoMessage(
      id: id,
      accountId: accountId,
      mailboxId: mailboxId ?? this.mailboxId,
      sender: sender,
      email: email,
      subject: subject,
      preview: preview,
      body: body,
      plainText: plainText,
      time: time,
      unread: unread ?? this.unread,
      flagged: flagged ?? this.flagged,
      hasAttachment: hasAttachment,
      attachments: attachments,
      draft: draft,
      editableDraft: editableDraft,
      draftSynchronized: draftSynchronized ?? this.draftSynchronized,
      draftTo: draftTo,
      draftCc: draftCc,
      draftBcc: draftBcc,
      toRecipients: toRecipients,
      ccRecipients: ccRecipients,
      bccRecipients: bccRecipients,
      editorDeltaJson: editorDeltaJson,
    );
  }
}

class MailAttachmentData {
  const MailAttachmentData({
    required this.id,
    required this.fileName,
    required this.contentType,
    required this.sizeBytes,
    required this.availableLocally,
  });

  final String id;
  final String fileName;
  final String contentType;
  final int sizeBytes;
  final bool availableLocally;
}

class LocalCalendarItem {
  const LocalCalendarItem({
    required this.id,
    required this.title,
    required this.startsAt,
    required this.endsAt,
    this.location,
  });

  final String id;
  final String title;
  final DateTime startsAt;
  final DateTime endsAt;
  final String? location;

  int get day => startsAt.day;

  String get time => '${_clock(startsAt)}–${_clock(endsAt)}';
}

class LocalTaskItem {
  const LocalTaskItem({
    required this.id,
    required this.title,
    required this.dueAt,
    required this.done,
  });

  final String id;
  final String title;
  final DateTime? dueAt;
  final bool done;

  String get due {
    final value = dueAt;
    if (value == null) return done ? 'Erledigt' : 'Ohne Datum';
    final now = DateTime.now();
    final date = DateTime(value.year, value.month, value.day);
    final today = DateTime(now.year, now.month, now.day);
    final difference = date.difference(today).inDays;
    if (difference == 0) return 'Heute';
    if (difference == 1) return 'Morgen';
    return '${value.day.toString().padLeft(2, '0')}.'
        '${value.month.toString().padLeft(2, '0')}.';
  }

  LocalTaskItem copyWith({bool? done}) {
    return LocalTaskItem(
      id: id,
      title: title,
      dueAt: dueAt,
      done: done ?? this.done,
    );
  }
}

class LocalContactItem {
  const LocalContactItem({
    required this.id,
    required this.name,
    required this.email,
  });

  final String id;
  final String name;
  final String email;
}

class MailAccountConfig {
  const MailAccountConfig({
    required this.id,
    required this.displayName,
    required this.email,
    required this.imapHost,
    required this.imapPort,
    required this.imapSecurity,
    required this.imapUsername,
    required this.smtpHost,
    required this.smtpPort,
    required this.smtpSecurity,
    required this.smtpUsername,
    this.authentication = 'password',
    this.oauthProvider,
    this.lastSyncAt,
  });

  final String id;
  final String displayName;
  final String email;
  final String imapHost;
  final int imapPort;
  final String imapSecurity;
  final String imapUsername;
  final String smtpHost;
  final int smtpPort;
  final String smtpSecurity;
  final String smtpUsername;
  final String authentication;
  final String? oauthProvider;
  final DateTime? lastSyncAt;
}

class WorkspaceDataSnapshot {
  const WorkspaceDataSnapshot({
    required this.folders,
    this.favoriteFolderIds = const [],
    this.darkModeEnabled = false,
    required this.messages,
    required this.calendarEvents,
    required this.tasks,
    required this.contacts,
    required this.mailAccounts,
    required this.pendingMailOperations,
    this.syncWarnings = const [],
    this.catalogMessagesRemaining = 0,
    this.deltaMailboxesSynchronized = 0,
    this.fullMailboxesReconciled = 0,
    this.qresyncMailboxesSynchronized = 0,
  });

  final List<MailFolder> folders;
  final List<String> favoriteFolderIds;
  final bool darkModeEnabled;
  final List<DemoMessage> messages;
  final List<LocalCalendarItem> calendarEvents;
  final List<LocalTaskItem> tasks;
  final List<LocalContactItem> contacts;
  final List<MailAccountConfig> mailAccounts;
  final int pendingMailOperations;
  final List<String> syncWarnings;
  final int catalogMessagesRemaining;
  final int deltaMailboxesSynchronized;
  final int fullMailboxesReconciled;
  final int qresyncMailboxesSynchronized;
}

class DraftSyncOutcome {
  const DraftSyncOutcome({
    required this.synchronized,
    required this.pending,
    this.warnings = const [],
  });

  final int synchronized;
  final int pending;
  final List<String> warnings;
}

class MailboxIdleOutcome {
  const MailboxIdleOutcome({
    required this.idleSupported,
    required this.changed,
  });

  final bool idleSupported;
  final bool changed;
}

/// Source of message view models for the workspace.
abstract interface class MailDataSource {
  const MailDataSource();

  List<MailFolder> get folders;
  List<String> get favoriteFolderIds;
  bool get darkModeEnabled;
  List<DemoMessage> get messages;
  List<LocalCalendarItem> get calendarEvents;
  List<LocalTaskItem> get tasks;
  List<LocalContactItem> get contacts;
  List<MailAccountConfig> get mailAccounts;
  int get pendingMailOperations;
  bool get isPersistent;
  bool get automaticSynchronizationEnabled;
  Future<MailboxIdleOutcome> waitForMailboxChange(
    String mailboxId, {
    Duration timeout = const Duration(seconds: 110),
  });

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
  });

  Future<void> exportAttachment(String attachmentId, String destinationPath);
  Future<void> exportProfile(String destinationPath, String password);
  Future<WorkspaceDataSnapshot> importProfile(
    String sourcePath,
    String password,
  );
  Future<List<DemoMessage>> searchMessages(
    String query, {
    bool includeContent = false,
  });

  /// Returns `null` when the exact IMAP UID was removed from the server and
  /// the stale local catalogue entry was deleted as part of the request.
  Future<DemoMessage?> loadMessageContent(DemoMessage message);
  Future<List<DemoMessage>> loadMailboxMessages(
    String mailboxId, {
    required int offset,
    int limit = 100,
  });

  Future<int> updateMessage(DemoMessage message);
  Future<void> createFolder(MailFolder folder);
  Future<void> renameFolder(MailFolder folder);
  Future<void> deleteFolder(String folderId, String fallbackFolderId);
  Future<void> saveFavoriteFolders(List<String> folderIds);
  Future<void> saveDarkMode(bool enabled);
  Future<void> saveCalendarEvent(LocalCalendarItem event);
  Future<void> saveTask(LocalTaskItem task);
  Future<void> saveContact(LocalContactItem contact);
  Future<void> testAccount(MailAccountConfig account, String password);
  Future<void> saveAccount(MailAccountConfig account, String password);
  Future<void> testOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  );
  Future<void> saveOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  );
  Future<WorkspaceDataSnapshot> deleteAccount(String accountId);
  Future<WorkspaceDataSnapshot> synchronizeAccounts();
  Future<DraftSyncOutcome> synchronizeDrafts(String accountId);
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
  });
}

/// Deterministic local data used while storage and synchronization are built.
class DemoMailDataSource implements MailDataSource {
  const DemoMailDataSource();

  @override
  List<MailFolder> get folders => demoFolders;

  @override
  List<String> get favoriteFolderIds => demoFolders
      .where(
        (folder) => const {'inbox', 'drafts', 'sent'}.contains(folder.role),
      )
      .take(3)
      .map((folder) => folder.id)
      .toList(growable: false);

  @override
  bool get darkModeEnabled => false;

  @override
  List<DemoMessage> get messages => demoMessages;

  @override
  List<LocalCalendarItem> get calendarEvents => demoCalendarEvents;

  @override
  List<LocalTaskItem> get tasks => demoTasks;

  @override
  List<LocalContactItem> get contacts => demoContacts;

  @override
  List<MailAccountConfig> get mailAccounts => const [];

  @override
  int get pendingMailOperations => 0;

  @override
  bool get isPersistent => false;

  @override
  bool get automaticSynchronizationEnabled => false;

  @override
  Future<MailboxIdleOutcome> waitForMailboxChange(
    String mailboxId, {
    Duration timeout = const Duration(seconds: 110),
  }) async => const MailboxIdleOutcome(idleSupported: false, changed: false);

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
  }) async => message;

  @override
  Future<void> exportAttachment(
    String attachmentId,
    String destinationPath,
  ) async {}

  @override
  Future<void> exportProfile(String destinationPath, String password) async {}

  @override
  Future<WorkspaceDataSnapshot> importProfile(
    String sourcePath,
    String password,
  ) async {
    return WorkspaceDataSnapshot(
      folders: demoFolders,
      favoriteFolderIds: favoriteFolderIds,
      messages: demoMessages,
      calendarEvents: demoCalendarEvents,
      tasks: demoTasks,
      contacts: demoContacts,
      mailAccounts: const [],
      pendingMailOperations: 0,
    );
  }

  @override
  Future<List<DemoMessage>> searchMessages(
    String query, {
    bool includeContent = false,
  }) async {
    final normalized = query.trim().toLowerCase();
    if (normalized.isEmpty) return const [];
    return demoMessages
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
  Future<DemoMessage?> loadMessageContent(DemoMessage message) async => message;

  @override
  Future<List<DemoMessage>> loadMailboxMessages(
    String mailboxId, {
    required int offset,
    int limit = 100,
  }) async {
    return demoMessages
        .where((message) => message.mailboxId == mailboxId)
        .skip(offset)
        .take(limit)
        .toList(growable: false);
  }

  @override
  Future<int> updateMessage(DemoMessage message) async => 0;

  @override
  Future<void> createFolder(MailFolder folder) async {}

  @override
  Future<void> renameFolder(MailFolder folder) async {}

  @override
  Future<void> deleteFolder(String folderId, String fallbackFolderId) async {}

  @override
  Future<void> saveFavoriteFolders(List<String> folderIds) async {}

  @override
  Future<void> saveDarkMode(bool enabled) async {}

  @override
  Future<void> saveCalendarEvent(LocalCalendarItem event) async {}

  @override
  Future<void> saveTask(LocalTaskItem task) async {}

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
    return WorkspaceDataSnapshot(
      folders: demoFolders,
      favoriteFolderIds: favoriteFolderIds,
      messages: demoMessages,
      calendarEvents: demoCalendarEvents,
      tasks: demoTasks,
      contacts: demoContacts,
      mailAccounts: const [],
      pendingMailOperations: 0,
      syncWarnings: const [],
    );
  }

  @override
  Future<WorkspaceDataSnapshot> synchronizeAccounts() async {
    return WorkspaceDataSnapshot(
      folders: demoFolders,
      favoriteFolderIds: favoriteFolderIds,
      messages: demoMessages,
      calendarEvents: demoCalendarEvents,
      tasks: demoTasks,
      contacts: demoContacts,
      mailAccounts: const [],
      pendingMailOperations: 0,
      syncWarnings: const [],
    );
  }

  @override
  Future<DraftSyncOutcome> synchronizeDrafts(String accountId) async {
    return const DraftSyncOutcome(synchronized: 0, pending: 0);
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
  }) async => 'demo';
}

class RustMailDataSource implements MailDataSource {
  const RustMailDataSource._({
    required this.databasePath,
    required this.folders,
    required this.favoriteFolderIds,
    required this.darkModeEnabled,
    required this.messages,
    required this.calendarEvents,
    required this.tasks,
    required this.contacts,
    required this.mailAccounts,
    required this.pendingMailOperations,
  });

  static Future<RustMailDataSource> open(String databasePath) async {
    final snapshot = await rust.openWorkspace(databasePath: databasePath);
    final data = _workspaceData(snapshot);
    return RustMailDataSource._(
      databasePath: databasePath,
      folders: data.folders,
      favoriteFolderIds: data.favoriteFolderIds,
      darkModeEnabled: data.darkModeEnabled,
      messages: data.messages,
      calendarEvents: data.calendarEvents,
      tasks: data.tasks,
      contacts: data.contacts,
      mailAccounts: data.mailAccounts,
      pendingMailOperations: data.pendingMailOperations,
    );
  }

  @override
  final List<MailFolder> folders;

  @override
  final List<String> favoriteFolderIds;

  @override
  final bool darkModeEnabled;

  @override
  final List<DemoMessage> messages;

  @override
  final List<LocalCalendarItem> calendarEvents;

  @override
  final List<LocalTaskItem> tasks;

  @override
  final List<LocalContactItem> contacts;

  @override
  final List<MailAccountConfig> mailAccounts;

  @override
  final int pendingMailOperations;

  final String databasePath;

  @override
  bool get isPersistent => true;

  @override
  bool get automaticSynchronizationEnabled => true;

  @override
  Future<MailboxIdleOutcome> waitForMailboxChange(
    String mailboxId, {
    Duration timeout = const Duration(seconds: 110),
  }) async {
    final result = await rust.waitForMailboxIdleChange(
      databasePath: databasePath,
      mailboxId: mailboxId,
      timeoutSeconds: timeout.inSeconds,
    );
    return MailboxIdleOutcome(
      idleSupported: result.idleSupported,
      changed: result.changed,
    );
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
    final saved = await rust.saveLocalMessage(
      databasePath: databasePath,
      input: rust.LocalMessageInput(
        id: message.id,
        accountId: message.accountId,
        mailboxId: message.mailboxId,
        sender: message.sender,
        email: message.email,
        subject: message.subject,
        preview: message.preview,
        plainText: plainText,
        htmlText: htmlText,
        attachmentPaths: attachmentPaths,
        retainedAttachmentIds: retainedAttachmentIds,
        draftTo: draftTo,
        draftCc: draftCc,
        draftBcc: draftBcc,
        editorDeltaJson: editorDeltaJson,
        receivedAtMs: DateTime.now().millisecondsSinceEpoch,
        unread: message.unread,
        flagged: message.flagged,
        draft: draft,
        hasAttachment: message.hasAttachment,
      ),
    );
    return _messageData(saved);
  }

  @override
  Future<void> exportAttachment(String attachmentId, String destinationPath) {
    return rust.exportAttachment(
      databasePath: databasePath,
      attachmentId: attachmentId,
      destinationPath: destinationPath,
    );
  }

  @override
  Future<void> exportProfile(String destinationPath, String password) {
    return rust.exportProfile(
      databasePath: databasePath,
      destinationPath: destinationPath,
      password: password,
    );
  }

  @override
  Future<WorkspaceDataSnapshot> importProfile(
    String sourcePath,
    String password,
  ) async {
    final snapshot = await rust.importProfile(
      databasePath: databasePath,
      sourcePath: sourcePath,
      password: password,
    );
    return _workspaceData(snapshot);
  }

  @override
  Future<List<DemoMessage>> searchMessages(
    String query, {
    bool includeContent = false,
  }) async {
    final results = await rust.searchProfileMessages(
      databasePath: databasePath,
      query: query,
      includeContent: includeContent,
      limit: 100,
    );
    return results.map(_messageData).toList(growable: false);
  }

  @override
  Future<DemoMessage?> loadMessageContent(DemoMessage message) async {
    final loaded = await rust.loadRemoteMessageContent(
      databasePath: databasePath,
      messageId: message.id,
    );
    return loaded == null ? null : _messageData(loaded);
  }

  @override
  Future<List<DemoMessage>> loadMailboxMessages(
    String mailboxId, {
    required int offset,
    int limit = 100,
  }) async {
    final page = await rust.loadMailboxMessages(
      databasePath: databasePath,
      mailboxId: mailboxId,
      offset: offset,
      limit: limit,
    );
    return page.map(_messageData).toList(growable: false);
  }

  @override
  Future<int> updateMessage(DemoMessage message) {
    return rust.updateLocalMessage(
      databasePath: databasePath,
      messageId: message.id,
      mailboxId: message.mailboxId,
      unread: message.unread,
      flagged: message.flagged,
    );
  }

  @override
  Future<void> createFolder(MailFolder folder) {
    return rust.createLocalMailbox(
      databasePath: databasePath,
      mailboxId: folder.id,
      displayName: folder.displayName,
    );
  }

  @override
  Future<void> renameFolder(MailFolder folder) {
    return rust.renameLocalMailbox(
      databasePath: databasePath,
      mailboxId: folder.id,
      displayName: folder.displayName,
    );
  }

  @override
  Future<void> deleteFolder(String folderId, String fallbackFolderId) {
    return rust.deleteLocalMailbox(
      databasePath: databasePath,
      mailboxId: folderId,
      fallbackMailboxId: fallbackFolderId,
    );
  }

  @override
  Future<void> saveFavoriteFolders(List<String> folderIds) {
    return rust.saveFavoriteMailboxes(
      databasePath: databasePath,
      mailboxIds: folderIds,
    );
  }

  @override
  Future<void> saveDarkMode(bool enabled) {
    return rust.saveDarkMode(databasePath: databasePath, enabled: enabled);
  }

  @override
  Future<void> saveCalendarEvent(LocalCalendarItem event) {
    return rust.saveLocalCalendarEvent(
      databasePath: databasePath,
      input: rust.LocalCalendarEventInput(
        id: event.id,
        title: event.title,
        startsAtMs: event.startsAt.millisecondsSinceEpoch,
        endsAtMs: event.endsAt.millisecondsSinceEpoch,
        location: event.location,
      ),
    );
  }

  @override
  Future<void> saveTask(LocalTaskItem task) {
    return rust.saveLocalTask(
      databasePath: databasePath,
      input: rust.LocalTaskInput(
        id: task.id,
        title: task.title,
        dueAtMs: task.dueAt?.millisecondsSinceEpoch,
        completed: task.done,
      ),
    );
  }

  @override
  Future<void> saveContact(LocalContactItem contact) {
    return rust.saveLocalContact(
      databasePath: databasePath,
      input: rust.LocalContactInput(
        id: contact.id,
        name: contact.name,
        email: contact.email,
      ),
    );
  }

  @override
  Future<void> testAccount(MailAccountConfig account, String password) {
    return rust.testMailAccountConnection(
      input: _mailAccountInput(account),
      password: password,
    );
  }

  @override
  Future<void> saveAccount(MailAccountConfig account, String password) {
    return rust.saveMailAccount(
      databasePath: databasePath,
      input: _mailAccountInput(account),
      password: password,
    );
  }

  @override
  Future<void> testOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  ) {
    return rust.testOauthMailAccountConnection(
      input: _mailAccountInput(account),
      accessToken: tokens.accessToken,
    );
  }

  @override
  Future<void> saveOAuthAccount(
    MailAccountConfig account,
    MailOAuthTokens tokens,
  ) {
    return rust.saveOauthMailAccount(
      databasePath: databasePath,
      input: _mailAccountInput(account),
      tokens: rust.OAuthTokenInput(
        provider: tokens.provider.storageName,
        clientId: tokens.clientId,
        accessToken: tokens.accessToken,
        refreshToken: tokens.refreshToken,
        expiresAtMs: tokens.expiresAt.millisecondsSinceEpoch,
        tokenEndpoint: tokens.tokenEndpoint,
        scopes: tokens.scopes,
      ),
    );
  }

  @override
  Future<WorkspaceDataSnapshot> deleteAccount(String accountId) async {
    final snapshot = await rust.deleteMailAccount(
      databasePath: databasePath,
      accountId: accountId,
    );
    return _workspaceData(snapshot);
  }

  @override
  Future<WorkspaceDataSnapshot> synchronizeAccounts() async {
    final snapshot = await rust.synchronizeMailAccounts(
      databasePath: databasePath,
    );
    return _workspaceData(snapshot);
  }

  @override
  Future<DraftSyncOutcome> synchronizeDrafts(String accountId) async {
    final result = await rust.synchronizeMailAccountDrafts(
      databasePath: databasePath,
      accountId: accountId,
    );
    return DraftSyncOutcome(
      synchronized: result.synchronized,
      pending: result.pending,
      warnings: result.warnings,
    );
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
  }) {
    return rust.sendAccountMessage(
      databasePath: databasePath,
      input: rust.OutgoingMessageInput(
        accountId: accountId,
        to: to,
        cc: cc,
        bcc: bcc,
        subject: subject,
        plainText: plainText,
        htmlText: htmlText,
        attachmentPaths: attachmentPaths,
        storedAttachmentIds: storedAttachmentIds,
        highImportance: highImportance,
      ),
    );
  }
}

WorkspaceDataSnapshot _workspaceData(rust.WorkspaceSnapshot snapshot) {
  return WorkspaceDataSnapshot(
    folders: snapshot.mailboxes
        .map(
          (folder) => MailFolder(
            id: folder.id,
            accountId: folder.accountId,
            displayName: folder.displayName,
            role: folder.role,
            unreadCount: folder.unreadCount,
            totalCount: folder.totalCount,
          ),
        )
        .toList(growable: false),
    favoriteFolderIds: snapshot.favoriteMailboxIds,
    darkModeEnabled: snapshot.darkModeEnabled,
    messages: snapshot.messages.map(_messageData).toList(growable: false),
    calendarEvents: snapshot.calendarEvents
        .map(
          (event) => LocalCalendarItem(
            id: event.id,
            title: event.title,
            startsAt: DateTime.fromMillisecondsSinceEpoch(
              event.startsAtMs,
            ).toLocal(),
            endsAt: DateTime.fromMillisecondsSinceEpoch(
              event.endsAtMs,
            ).toLocal(),
            location: event.location,
          ),
        )
        .toList(growable: false),
    tasks: snapshot.tasks
        .map(
          (task) => LocalTaskItem(
            id: task.id,
            title: task.title,
            dueAt: task.dueAtMs == null
                ? null
                : DateTime.fromMillisecondsSinceEpoch(task.dueAtMs!).toLocal(),
            done: task.completed,
          ),
        )
        .toList(growable: false),
    contacts: snapshot.contacts
        .map(
          (contact) => LocalContactItem(
            id: contact.id,
            name: contact.name,
            email: contact.email,
          ),
        )
        .toList(growable: false),
    mailAccounts: snapshot.mailAccounts
        .map(
          (account) => MailAccountConfig(
            id: account.id,
            displayName: account.displayName,
            email: account.email,
            imapHost: account.imapHost,
            imapPort: account.imapPort,
            imapSecurity: account.imapSecurity,
            imapUsername: account.imapUsername,
            smtpHost: account.smtpHost,
            smtpPort: account.smtpPort,
            smtpSecurity: account.smtpSecurity,
            smtpUsername: account.smtpUsername,
            authentication: account.authentication,
            oauthProvider: account.oauthProvider,
            lastSyncAt: account.lastSyncAtMs == null
                ? null
                : DateTime.fromMillisecondsSinceEpoch(
                    account.lastSyncAtMs!,
                  ).toLocal(),
          ),
        )
        .toList(growable: false),
    pendingMailOperations: snapshot.pendingMailOperations,
    syncWarnings: snapshot.syncWarnings,
    catalogMessagesRemaining: snapshot.catalogMessagesRemaining,
    deltaMailboxesSynchronized: snapshot.deltaMailboxesSynchronized,
    fullMailboxesReconciled: snapshot.fullMailboxesReconciled,
    qresyncMailboxesSynchronized: snapshot.qresyncMailboxesSynchronized,
  );
}

DemoMessage _messageData(rust.MessageDto message) {
  return DemoMessage(
    id: message.id,
    accountId: message.accountId,
    mailboxId: message.mailboxId,
    sender: message.sender,
    email: message.email,
    subject: message.subject,
    preview: message.preview,
    body: message.body,
    plainText: message.plainText,
    time: _formatTimestamp(message.receivedAtMs),
    unread: message.unread,
    flagged: message.flagged,
    draft: message.draft,
    editableDraft: message.editableDraft,
    draftSynchronized: message.draftSynchronized,
    draftTo: message.draftTo,
    draftCc: message.draftCc,
    draftBcc: message.draftBcc,
    toRecipients: message.toRecipients,
    ccRecipients: message.ccRecipients,
    bccRecipients: message.bccRecipients,
    editorDeltaJson: message.editorDeltaJson,
    hasAttachment: message.hasAttachment,
    attachments: message.attachments
        .map(
          (attachment) => MailAttachmentData(
            id: attachment.id,
            fileName: attachment.fileName,
            contentType: attachment.contentType,
            sizeBytes: attachment.sizeBytes,
            availableLocally: attachment.availableLocally,
          ),
        )
        .toList(growable: false),
  );
}

rust.MailAccountInput _mailAccountInput(MailAccountConfig account) {
  return rust.MailAccountInput(
    id: account.id,
    displayName: account.displayName,
    email: account.email,
    imapHost: account.imapHost,
    imapPort: account.imapPort,
    imapSecurity: account.imapSecurity,
    imapUsername: account.imapUsername,
    smtpHost: account.smtpHost,
    smtpPort: account.smtpPort,
    smtpSecurity: account.smtpSecurity,
    smtpUsername: account.smtpUsername,
  );
}

String _clock(DateTime value) {
  return '${value.hour.toString().padLeft(2, '0')}:'
      '${value.minute.toString().padLeft(2, '0')}';
}

String _formatTimestamp(int milliseconds) {
  final value = DateTime.fromMillisecondsSinceEpoch(milliseconds).toLocal();
  final now = DateTime.now();
  final valueDay = DateTime(value.year, value.month, value.day);
  final today = DateTime(now.year, now.month, now.day);
  final difference = today.difference(valueDay).inDays;
  if (difference == 0) {
    return '${value.hour.toString().padLeft(2, '0')}:'
        '${value.minute.toString().padLeft(2, '0')}';
  }
  if (difference == 1) return 'Gestern';
  return '${value.day.toString().padLeft(2, '0')}.'
      '${value.month.toString().padLeft(2, '0')}.';
}

const demoFolders = <MailFolder>[
  MailFolder(
    id: 'personal.inbox',
    displayName: 'Posteingang',
    role: 'inbox',
    unreadCount: 2,
    totalCount: 5,
  ),
  MailFolder(
    id: 'personal.drafts',
    displayName: 'Entwürfe',
    role: 'drafts',
    unreadCount: 0,
    totalCount: 1,
  ),
  MailFolder(
    id: 'personal.sent',
    displayName: 'Gesendet',
    role: 'sent',
    unreadCount: 0,
    totalCount: 0,
  ),
  MailFolder(
    id: 'personal.archive',
    displayName: 'Archiv',
    role: 'archive',
    unreadCount: 0,
    totalCount: 0,
  ),
  MailFolder(
    id: 'personal.trash',
    displayName: 'Papierkorb',
    role: 'trash',
    unreadCount: 0,
    totalCount: 0,
  ),
  MailFolder(
    id: 'personal.junk',
    displayName: 'Spam',
    role: 'junk',
    unreadCount: 0,
    totalCount: 0,
  ),
];

const demoMessages = <DemoMessage>[
  DemoMessage(
    id: 'demo.welcome',
    mailboxId: 'personal.inbox',
    sender: 'MAICENTA Team',
    email: 'hello@maicenta.local',
    subject: 'Willkommen bei MAICENTA',
    preview: 'Dein lokaler Workspace ist bereit für den ersten Rundgang.',
    body:
        '<p>Hallo,</p><p>willkommen beim ersten MAICENTA-Prototypen. Diese '
        'Oberfläche zeigt die geplante Arbeitsweise für E-Mail, Kalender, '
        'Aufgaben und Kontakte.</p><p>Der Prototyp verwendet ausschließlich '
        'Beispieldaten. Konten und Synchronisierung folgen in den nächsten '
        'Entwicklungsschritten.</p><p>Viele Grüße<br>Das MAICENTA Team</p>',
    time: '10:42',
    unread: true,
  ),
  DemoMessage(
    id: 'demo.planning',
    mailboxId: 'personal.inbox',
    sender: 'Anna Schneider',
    email: 'anna@example.org',
    subject: 'Projektplanung für diese Woche',
    preview: 'Ich habe die offenen Punkte für unseren Termin zusammengefasst.',
    body:
        '<p>Hallo,</p><p>ich habe die offenen Punkte für unseren Termin am '
        'Donnerstag zusammengefasst. Im Anhang findest du die aktuelle '
        'Übersicht.</p><p>Viele Grüße<br>Anna</p>',
    time: '09:18',
    unread: true,
    flagged: true,
    hasAttachment: true,
  ),
  DemoMessage(
    id: 'demo.calendar-reminder',
    mailboxId: 'personal.inbox',
    sender: 'Kalender',
    email: 'calendar@maicenta.local',
    subject: 'Erinnerung: Team-Stand-up',
    preview: 'Der Termin beginnt morgen um 09:30 Uhr.',
    body:
        '<h3>Erinnerung</h3><p><strong>Team-Stand-up</strong><br>Morgen, '
        '09:30–10:00<br>Besprechungsraum Nord</p>',
    time: 'Gestern',
  ),
  DemoMessage(
    id: 'demo.design',
    mailboxId: 'personal.inbox',
    sender: 'Jonas Weber',
    email: 'jonas@example.org',
    subject: 'Re: Design-Entwurf',
    preview:
        'Die klare Navigation gefällt mir. Zwei Anmerkungen habe ich noch.',
    body:
        '<p>Hallo,</p><p>die klare Navigation gefällt mir. Zwei kleine '
        'Anmerkungen habe ich noch direkt im Dokument ergänzt.</p><p>Beste '
        'Grüße<br>Jonas</p>',
    time: 'Gestern',
    hasAttachment: true,
  ),
  DemoMessage(
    id: 'demo.open-source-weekly',
    mailboxId: 'personal.inbox',
    sender: 'Open Source Weekly',
    email: 'newsletter@example.org',
    subject: 'Local-first software in practice',
    preview: 'This week: resilient sync, portable data and open protocols.',
    body:
        '<div style="max-width:640px;font-family:Arial;color:#242424">'
        '<div style="background-color:#0f5fae;color:#ffffff;padding:18px">'
        '<strong style="font-size:20px">Open Source Weekly</strong></div>'
        '<div style="padding:20px;border:1px solid #d5d9de">'
        '<h2 style="color:#0f5fae">Local-first software in practice</h2>'
        '<p>This week we look at <strong>resilient synchronization</strong>, '
        'portable user data, and open protocols.</p>'
        '<table width="100%" cellpadding="8" style="border-collapse:collapse">'
        '<tr><td style="background-color:#eef5fb">Offline-first</td>'
        '<td style="background-color:#f7f7f7">Open standards</td></tr>'
        '</table><p><a href="https://example.org/article">Read the article</a>'
        '</p></div></div>',
    time: 'Mo',
  ),
];

final demoCalendarEvents = <LocalCalendarItem>[
  LocalCalendarItem(
    id: 'demo.calendar.standup',
    title: 'Team-Stand-up',
    startsAt: DateTime(2026, 7, 28, 9, 30),
    endsAt: DateTime(2026, 7, 28, 10),
    location: 'Besprechungsraum Nord',
  ),
  LocalCalendarItem(
    id: 'demo.calendar.planning',
    title: 'Projektplanung',
    startsAt: DateTime(2026, 7, 30, 14),
    endsAt: DateTime(2026, 7, 30, 15),
  ),
];

final demoTasks = <LocalTaskItem>[
  LocalTaskItem(
    id: 'demo.task.imap',
    title: 'IMAP-Testkonto vorbereiten',
    dueAt: DateTime(2026, 8, 3),
    done: false,
  ),
  LocalTaskItem(
    id: 'demo.task.architecture',
    title: 'Architekturfeedback einarbeiten',
    dueAt: DateTime(2026, 8, 4),
    done: false,
  ),
  const LocalTaskItem(
    id: 'demo.task.prototype',
    title: 'Desktop-Prototyp prüfen',
    dueAt: null,
    done: true,
  ),
];

const demoContacts = <LocalContactItem>[
  LocalContactItem(
    id: 'demo.contact.anna',
    name: 'Anna Schneider',
    email: 'anna@example.org',
  ),
  LocalContactItem(
    id: 'demo.contact.jonas',
    name: 'Jonas Weber',
    email: 'jonas@example.org',
  ),
  LocalContactItem(
    id: 'demo.contact.team',
    name: 'MAICENTA Team',
    email: 'hello@maicenta.local',
  ),
];
