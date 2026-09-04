import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/mail/folder_tree.dart';
import 'package:maicenta/features/mail/mail_data.dart';

MailFolder _folder(String id, String name, {String role = 'custom'}) =>
    MailFolder(
      id: id,
      accountId: 'work',
      displayName: name,
      role: role,
      unreadCount: 0,
      totalCount: 0,
    );

void main() {
  test('strips IMAP and Graph inbox namespaces from custom folder paths', () {
    final imap = customFolderPath('INBOX.Projects');
    expect(imap.segments, ['Projects']);
    expect(imap.underInbox, isTrue);

    final graph = customFolderPath(
      'Posteingang/12 Level Trade',
      inboxName: 'Posteingang',
    );
    expect(graph.segments, ['12 Level Trade']);
    expect(graph.underInbox, isTrue);

    final nested = customFolderPath('Synchronisierungsprobleme/Konflikte');
    expect(nested.segments, ['Synchronisierungsprobleme', 'Konflikte']);
    expect(nested.underInbox, isFalse);
    expect(nested.leaf, 'Konflikte');

    // IMAP dots stay part of the name; only slashes separate levels.
    expect(customFolderPath('Archive.2024').segments, ['Archive.2024']);
  });

  test('nests inbox children after the inbox and indents by depth', () {
    final tree = buildFolderTree([
      _folder('inbox', 'Posteingang', role: 'inbox'),
      _folder('drafts', 'Entwürfe', role: 'drafts'),
      _folder('trash', 'Gelöschte Elemente', role: 'trash'),
      _folder('outbox', 'Postausgang'),
      _folder('trade', 'Posteingang/12 Level Trade'),
      _folder('open', 'Posteingang/Offen'),
      _folder('sync', 'Synchronisierungsprobleme'),
      _folder('sync-conflicts', 'Synchronisierungsprobleme/Konflikte'),
      _folder('sync-local', 'Synchronisierungsprobleme/Lokale Fehler'),
    ]);

    expect(tree.map((entry) => entry.folder.id).toList(), [
      'inbox',
      'trade',
      'open',
      'drafts',
      'trash',
      'outbox',
      'sync',
      'sync-conflicts',
      'sync-local',
    ]);
    expect(tree.map((entry) => entry.depth).toList(), [
      0,
      1,
      1,
      0,
      0,
      0,
      0,
      1,
      1,
    ]);
    expect(tree[1].leafName, '12 Level Trade');
    expect(tree[1].path, '12 Level Trade');
    expect(tree[7].leafName, 'Konflikte');
    expect(tree[7].path, 'Synchronisierungsprobleme/Konflikte');
  });

  test('links children to their parent and hides collapsed subtrees', () {
    final tree = buildFolderTree([
      _folder('inbox', 'Posteingang', role: 'inbox'),
      _folder('trade', 'Posteingang/12 Level Trade'),
      _folder('deep', 'Posteingang/12 Level Trade/2026'),
      _folder('sync', 'Synchronisierungsprobleme'),
      _folder('sync-conflicts', 'Synchronisierungsprobleme/Konflikte'),
      _folder('orphan', 'Reports/Weekly/Archive'),
    ]);
    final byId = {for (final entry in tree) entry.folder.id: entry};

    expect(byId['trade']!.parentId, 'inbox');
    expect(byId['deep']!.parentId, 'trade');
    expect(byId['deep']!.depth, 2);
    expect(byId['sync-conflicts']!.parentId, 'sync');
    // A missing intermediate level does not orphan the folder into the void.
    expect(byId['orphan']!.parentId, isNull);
    expect(byId['orphan']!.depth, 0);
    expect(byId['orphan']!.leafName, 'Archive');
    expect(folderTreeParentIds(tree), {'inbox', 'trade', 'sync'});

    final collapsedInbox = visibleFolderTree(tree, {'inbox'});
    // Top-level custom folders sort by path, so "Reports/…" precedes "Sync…".
    expect(collapsedInbox.map((entry) => entry.folder.id), [
      'inbox',
      'orphan',
      'sync',
      'sync-conflicts',
    ]);
    final collapsedTrade = visibleFolderTree(tree, {'trade'});
    expect(collapsedTrade.map((entry) => entry.folder.id), contains('trade'));
    expect(
      collapsedTrade.map((entry) => entry.folder.id),
      isNot(contains('deep')),
    );
    expect(visibleFolderTree(tree, const {}), same(tree));
  });

  test('keeps custom folders visible when the account has no inbox', () {
    final tree = buildFolderTree([
      _folder('a', 'INBOX/Alpha'),
      _folder('b', 'Beta'),
    ]);
    expect(tree.map((entry) => entry.folder.id).toList(), ['a', 'b']);
    // Without an inbox row there is nothing to nest under.
    expect(tree.first.depth, 0);
    expect(tree.first.parentId, isNull);
  });
}
