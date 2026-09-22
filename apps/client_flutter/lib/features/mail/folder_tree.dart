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

/// Mutable tree node used while assembling the folder pane rows.
class _FolderNode {
  _FolderNode({
    required this.folder,
    required this.leafName,
    required this.path,
    required this.defaultIndex,
  });

  final MailFolder folder;
  final String leafName;
  final String path;

  /// Position in the provider-neutral default order, used for siblings the
  /// user has not arranged manually.
  final int defaultIndex;
  final List<_FolderNode> children = [];
}

/// Orders one account's folders as a tree: standard folders first in their
/// given order, custom folders below the inbox directly after it, remaining
/// custom folders afterwards, each sorted by path and nested under the
/// nearest existing ancestor.
///
/// [orderIds] is the user-defined order: siblings listed there come first in
/// that sequence, unlisted siblings follow in the default order. Because only
/// siblings are compared, one flat list serves every level of every account.
List<FolderTreeEntry> buildFolderTree(
  List<MailFolder> accountFolders, {
  List<String> orderIds = const [],
}) {
  final roots = <_FolderNode>[];
  _FolderNode? inboxNode;
  var nextIndex = 0;
  for (final folder in accountFolders) {
    if (folder.role == 'custom') continue;
    final node = _FolderNode(
      folder: folder,
      leafName: folder.displayName,
      path: folder.displayName,
      defaultIndex: nextIndex++,
    );
    roots.add(node);
    if (folder.role == 'inbox') inboxNode ??= node;
  }

  final inboxChildren = <(MailFolder, CustomFolderPath)>[];
  final others = <(MailFolder, CustomFolderPath)>[];
  for (final folder in accountFolders) {
    if (folder.role != 'custom') continue;
    final path = customFolderPath(
      folder.displayName,
      inboxName: inboxNode?.folder.displayName,
    );
    (path.underInbox ? inboxChildren : others).add((folder, path));
  }
  int byPath(
    (MailFolder, CustomFolderPath) left,
    (MailFolder, CustomFolderPath) right,
  ) => left.$2.joined.toLowerCase().compareTo(right.$2.joined.toLowerCase());
  inboxChildren.sort(byPath);
  others.sort(byPath);

  void attach(List<(MailFolder, CustomFolderPath)> group, _FolderNode? root) {
    final byLowerPath = <String, _FolderNode>{};
    for (final (folder, path) in group) {
      _FolderNode? parent;
      for (var length = path.segments.length - 1; length > 0; length -= 1) {
        parent =
            byLowerPath[path.segments.take(length).join('/').toLowerCase()];
        if (parent != null) break;
      }
      final node = _FolderNode(
        folder: folder,
        leafName: path.leaf,
        path: path.joined,
        defaultIndex: nextIndex++,
      );
      byLowerPath.putIfAbsent(path.joined.toLowerCase(), () => node);
      final target = parent ?? root;
      if (target == null) {
        roots.add(node);
      } else {
        target.children.add(node);
      }
    }
  }

  attach(inboxChildren, inboxNode);
  attach(others, null);

  final rank = <String, int>{};
  for (var index = 0; index < orderIds.length; index += 1) {
    rank.putIfAbsent(orderIds[index], () => index);
  }
  int compare(_FolderNode left, _FolderNode right) {
    final leftRank = rank[left.folder.id];
    final rightRank = rank[right.folder.id];
    if (leftRank != null && rightRank != null) {
      return leftRank.compareTo(rightRank);
    }
    if (leftRank != null) return -1;
    if (rightRank != null) return 1;
    return left.defaultIndex.compareTo(right.defaultIndex);
  }

  final tree = <FolderTreeEntry>[];
  void flatten(List<_FolderNode> nodes, int depth, String? parentId) {
    nodes.sort(compare);
    for (final node in nodes) {
      tree.add(
        FolderTreeEntry(
          folder: node.folder,
          depth: depth,
          leafName: node.leafName,
          path: node.path,
          parentId: parentId,
        ),
      );
      flatten(node.children, depth + 1, node.folder.id);
    }
  }

  flatten(roots, 0, null);
  return tree;
}

/// User-defined order after dropping [draggedId] onto [targetId].
///
/// Dragging downwards places the folder below the target, dragging upwards
/// above it, so every arrangement is reachable. Returns `null` when the two
/// folders are not siblings in [tree] or are the same folder; the caller
/// then leaves the order untouched. The result replaces [orderIds]: the
/// affected siblings are listed completely so their order no longer depends
/// on the default sort.
List<String>? folderOrderAfterMove({
  required List<FolderTreeEntry> tree,
  required List<String> orderIds,
  required String draggedId,
  required String targetId,
}) {
  if (draggedId == targetId) return null;
  final dragged = tree
      .where((entry) => entry.folder.id == draggedId)
      .firstOrNull;
  final target = tree.where((entry) => entry.folder.id == targetId).firstOrNull;
  if (dragged == null ||
      target == null ||
      dragged.parentId != target.parentId) {
    return null;
  }
  final siblings = [
    for (final entry in tree)
      if (entry.parentId == dragged.parentId) entry.folder.id,
  ];
  final from = siblings.indexOf(draggedId);
  final to = siblings.indexOf(targetId);
  siblings.removeAt(from);
  // After the removal the target sits at `to - 1` when the folder moves
  // down, so inserting at `to` lands below it; moving up lands above it.
  siblings.insert(to, draggedId);
  final siblingIds = siblings.toSet();
  return [...orderIds.where((id) => !siblingIds.contains(id)), ...siblings];
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
