import 'mail_data.dart';

/// One row of the folder pane: a folder with its nesting depth and the
/// path-relative labels used for display and tooltips.
class FolderTreeEntry {
  const FolderTreeEntry({
    required this.folder,
    required this.depth,
    required this.leafName,
    required this.path,
    this.parentId,
  });

  final MailFolder folder;

  /// Folder this entry is nested under, or `null` for top-level rows.
  final String? parentId;

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
/// custom folders afterwards, each sorted by path and nested under the
/// nearest existing ancestor.
List<FolderTreeEntry> buildFolderTree(List<MailFolder> accountFolders) {
  final inbox = accountFolders
      .where((folder) => folder.role == 'inbox')
      .firstOrNull;
  final inboxChildren = <(MailFolder, CustomFolderPath)>[];
  final others = <(MailFolder, CustomFolderPath)>[];
  for (final folder in accountFolders) {
    if (folder.role != 'custom') continue;
    final path = customFolderPath(
      folder.displayName,
      inboxName: inbox?.displayName,
    );
    (path.underInbox ? inboxChildren : others).add((folder, path));
  }
  int byPath(
    (MailFolder, CustomFolderPath) left,
    (MailFolder, CustomFolderPath) right,
  ) => left.$2.joined.toLowerCase().compareTo(right.$2.joined.toLowerCase());
  inboxChildren.sort(byPath);
  others.sort(byPath);

  List<FolderTreeEntry> nest(
    List<(MailFolder, CustomFolderPath)> group, {
    String? rootId,
    required int rootDepth,
  }) {
    final byLowerPath = <String, FolderTreeEntry>{};
    final entries = <FolderTreeEntry>[];
    for (final (folder, path) in group) {
      FolderTreeEntry? parent;
      for (var length = path.segments.length - 1; length > 0; length -= 1) {
        parent =
            byLowerPath[path.segments.take(length).join('/').toLowerCase()];
        if (parent != null) break;
      }
      final entry = FolderTreeEntry(
        folder: folder,
        depth: parent == null ? rootDepth : parent.depth + 1,
        leafName: path.leaf,
        path: path.joined,
        parentId: parent?.folder.id ?? rootId,
      );
      byLowerPath.putIfAbsent(path.joined.toLowerCase(), () => entry);
      entries.add(entry);
    }
    return entries;
  }

  final nestedInboxChildren = nest(
    inboxChildren,
    rootId: inbox?.id,
    rootDepth: inbox == null ? 0 : 1,
  );
  final nestedOthers = nest(others, rootDepth: 0);

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
    if (folder.role == 'inbox') tree.addAll(nestedInboxChildren);
  }
  if (inbox == null) tree.addAll(nestedInboxChildren);
  tree.addAll(nestedOthers);
  return tree;
}

/// Identifiers of every folder that has at least one nested child.
Set<String> folderTreeParentIds(List<FolderTreeEntry> tree) => {
  for (final entry in tree)
    if (entry.parentId != null) entry.parentId!,
};

/// Rows that remain visible when the given folders are collapsed: an entry is
/// hidden as soon as any of its ancestors is collapsed.
List<FolderTreeEntry> visibleFolderTree(
  List<FolderTreeEntry> tree,
  Set<String> collapsedFolderIds,
) {
  if (collapsedFolderIds.isEmpty) return tree;
  final parents = {for (final entry in tree) entry.folder.id: entry.parentId};
  bool hiddenByAncestor(FolderTreeEntry entry) {
    var parentId = entry.parentId;
    var guard = 0;
    while (parentId != null && guard < 64) {
      if (collapsedFolderIds.contains(parentId)) return true;
      parentId = parents[parentId];
      guard += 1;
    }
    return false;
  }

  return tree
      .where((entry) => !hiddenByAncestor(entry))
      .toList(growable: false);
}
