import 'mail_data.dart';

/// One row of the folder pane: a folder with its nesting depth and the
/// path-relative labels used for display and tooltips.
class FolderTreeEntry {
  const FolderTreeEntry({
    required this.folder,
    required this.depth,
    required this.leafName,
    required this.path,
  });

  final MailFolder folder;

  /// 0 for top-level folders, 1 for direct children, and so on.
  final int depth;

  /// Last path segment, shown as the row label for custom folders.
  final String leafName;

  /// Complete provider path without the inbox namespace, shown as tooltip.
  final String path;
}

/// Provider-neutral view of a custom folder's position in the hierarchy.
class CustomFolderPath {
  const CustomFolderPath({required this.segments, required this.underInbox});

  /// Path segments without the inbox namespace, never empty.
  final List<String> segments;

  /// Whether the folder lives below the account's inbox.
  final bool underInbox;

  String get leaf => segments.last;

  String get joined => segments.join('/');
}

/// Splits a custom folder's server name into hierarchy segments.
///
/// IMAP servers report `INBOX.Projects` or `INBOX/Projects`; Microsoft Graph
/// folders arrive as `Posteingang/Projekte`, using the inbox's own display
/// name as the first segment. Both forms become `[Projects]` under the inbox.
/// Only `/` separates further levels: IMAP `.` delimiters are ambiguous with
/// dots inside folder names and are therefore left untouched.
CustomFolderPath customFolderPath(String serverName, {String? inboxName}) {
  var name = serverName;
  var underInbox = false;
  for (final prefix in const ['INBOX.', 'INBOX/', 'INBOX\\']) {
    if (name.toUpperCase().startsWith(prefix)) {
      name = name.substring(prefix.length);
      underInbox = true;
      break;
    }
  }
  if (!underInbox && inboxName != null && inboxName.isNotEmpty) {
    final inboxPrefix = '${inboxName.toLowerCase()}/';
    if (name.toLowerCase().startsWith(inboxPrefix)) {
      name = name.substring(inboxPrefix.length);
      underInbox = true;
    }
  }
  final segments = name
      .split('/')
      .map((segment) => segment.trim())
      .where((segment) => segment.isNotEmpty)
      .toList(growable: false);
  return CustomFolderPath(
    segments: segments.isEmpty ? [serverName] : segments,
    underInbox: underInbox,
  );
}

/// Orders one account's folders as a tree: standard folders first in their
/// given order, custom folders below the inbox directly after it, remaining
/// custom folders afterwards, each sorted by path and indented by depth.
List<FolderTreeEntry> buildFolderTree(List<MailFolder> accountFolders) {
  final inbox = accountFolders
      .where((folder) => folder.role == 'inbox')
      .firstOrNull;
  final inboxChildren = <(FolderTreeEntry, String)>[];
  final others = <(FolderTreeEntry, String)>[];
  for (final folder in accountFolders) {
    if (folder.role != 'custom') continue;
    final path = customFolderPath(
      folder.displayName,
      inboxName: inbox?.displayName,
    );
    final entry = FolderTreeEntry(
      folder: folder,
      depth: (path.underInbox ? 1 : 0) + path.segments.length - 1,
      leafName: path.leaf,
      path: path.joined,
    );
    (path.underInbox ? inboxChildren : others).add((
      entry,
      path.joined.toLowerCase(),
    ));
  }
  int byPath((FolderTreeEntry, String) left, (FolderTreeEntry, String) right) =>
      left.$2.compareTo(right.$2);
  inboxChildren.sort(byPath);
  others.sort(byPath);

  final tree = <FolderTreeEntry>[];
  for (final folder in accountFolders) {
    if (folder.role == 'custom') continue;
    tree.add(
      FolderTreeEntry(
        folder: folder,
        depth: 0,
        leafName: folder.displayName,
        path: folder.displayName,
      ),
    );
    if (folder.role == 'inbox') {
      tree.addAll(inboxChildren.map((entry) => entry.$1));
    }
  }
  if (inbox == null) {
    tree.addAll(inboxChildren.map((entry) => entry.$1));
  }
  tree.addAll(others.map((entry) => entry.$1));
  return tree;
}
