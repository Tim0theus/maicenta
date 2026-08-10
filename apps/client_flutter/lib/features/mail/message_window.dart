import 'package:flutter/material.dart';
import 'package:flutter_widget_from_html_core/flutter_widget_from_html_core.dart';

import '../../app_theme.dart';
import 'mail_data.dart';
import 'mailbox_labels.dart';

const _outlookBlue = Color(0xFF0F6CBD);

Future<void> showMessageWindow(
  BuildContext context, {
  required DemoMessage message,
  required List<MailFolder> folders,
  required Future<void> Function(DemoMessage message) onReply,
  required Future<void> Function(DemoMessage message) onReplyAll,
  required Future<void> Function(DemoMessage message) onForward,
  required Future<void> Function(DemoMessage message) onEditDraft,
  required Future<DemoMessage?> Function(DemoMessage message) onUpdate,
  required Future<bool> Function(DemoMessage message, String mailboxId) onMove,
  required Future<void> Function(MailAttachmentData attachment)
  onSaveAttachment,
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (_) => Dialog.fullscreen(
      child: _MessageWindow(
        message: message,
        folders: folders,
        onReply: onReply,
        onReplyAll: onReplyAll,
        onForward: onForward,
        onEditDraft: onEditDraft,
        onUpdate: onUpdate,
        onMove: onMove,
        onSaveAttachment: onSaveAttachment,
      ),
    ),
  );
}

class _MessageWindow extends StatefulWidget {
  const _MessageWindow({
    required this.message,
    required this.folders,
    required this.onReply,
    required this.onReplyAll,
    required this.onForward,
    required this.onEditDraft,
    required this.onUpdate,
    required this.onMove,
    required this.onSaveAttachment,
  });

  final DemoMessage message;
  final List<MailFolder> folders;
  final Future<void> Function(DemoMessage message) onReply;
  final Future<void> Function(DemoMessage message) onReplyAll;
  final Future<void> Function(DemoMessage message) onForward;
  final Future<void> Function(DemoMessage message) onEditDraft;
  final Future<DemoMessage?> Function(DemoMessage message) onUpdate;
  final Future<bool> Function(DemoMessage message, String mailboxId) onMove;
  final Future<void> Function(MailAttachmentData attachment) onSaveAttachment;

  @override
  State<_MessageWindow> createState() => _MessageWindowState();
}

class _MessageWindowState extends State<_MessageWindow> {
  late DemoMessage message;
  String selectedTab = 'Nachricht';
  double zoom = 1;
  bool showDetails = true;
  bool busy = false;

  @override
  void initState() {
    super.initState();
    message = widget.message;
  }

  Future<void> updateMessage(DemoMessage updated) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      final saved = await widget.onUpdate(updated);
      if (mounted && saved != null) setState(() => message = saved);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> moveTo(String mailboxId, {bool close = false}) async {
    if (busy) return;
    setState(() => busy = true);
    try {
      final moved = await widget.onMove(message, mailboxId);
      if (!mounted || !moved) return;
      if (close) {
        Navigator.pop(context);
      } else {
        setState(() => message = message.copyWith(mailboxId: mailboxId));
      }
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  MailFolder? folderForRole(String role) {
    for (final folder in widget.folders) {
      if (folder.accountId == message.accountId && folder.role == role) {
        return folder;
      }
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    return Material(
      color: MaicentaPalette.of(context).window,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _titleBar(),
          _tabs(),
          _ribbon(),
          if (busy) const LinearProgressIndicator(minHeight: 2),
          Expanded(child: _content()),
          _statusBar(),
        ],
      ),
    );
  }

  Widget _titleBar() {
    return Container(
      key: const Key('classic-message-title-bar'),
      height: 39,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      padding: const EdgeInsets.only(left: 8),
      child: Row(
        children: [
          const Icon(Icons.mail_outline, size: 18, color: _outlookBlue),
          const SizedBox(width: 9),
          Expanded(
            child: Text(
              '${message.subject.isEmpty ? '(Kein Betreff)' : message.subject} – Nachricht',
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 12.5),
            ),
          ),
          IconButton(
            key: const Key('message-window-close'),
            tooltip: 'Schließen',
            onPressed: () => Navigator.pop(context),
            icon: const Icon(Icons.close, size: 19),
          ),
        ],
      ),
    );
  }

  Widget _tabs() {
    const tabs = ['Datei', 'Nachricht', 'Ansicht'];
    return Container(
      key: const Key('classic-message-tabs'),
      height: 31,
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          for (final tab in tabs)
            InkWell(
              key: Key('message-tab-$tab'),
              onTap: () => setState(() => selectedTab = tab),
              child: Container(
                width: tab == 'Nachricht' ? 88 : 67,
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  color: selectedTab == tab
                      ? MaicentaPalette.of(context).subtle
                      : MaicentaPalette.of(context).window,
                  border: Border(
                    bottom: BorderSide(
                      color: selectedTab == tab
                          ? _outlookBlue
                          : Colors.transparent,
                      width: 2,
                    ),
                  ),
                ),
                child: Text(tab, style: const TextStyle(fontSize: 11.5)),
              ),
            ),
        ],
      ),
    );
  }

  Widget _ribbon() {
    final groups = switch (selectedTab) {
      'Datei' => [
        _MessageRibbonGroup(
          label: 'Fenster',
          children: [
            _command(
              key: 'message-file-close',
              icon: Icons.close,
              label: 'Schließen',
              onTap: () => Navigator.pop(context),
            ),
          ],
        ),
        if (message.attachments.isNotEmpty)
          _MessageRibbonGroup(
            label: 'Anlagen',
            children: [
              _command(
                key: 'message-save-all-attachments',
                icon: Icons.download_outlined,
                label: 'Alle speichern',
                onTap: () async {
                  for (final attachment in message.attachments) {
                    await widget.onSaveAttachment(attachment);
                  }
                },
              ),
            ],
          ),
      ],
      'Ansicht' => [
        _MessageRibbonGroup(
          label: 'Zoom',
          children: [
            _command(
              key: 'message-zoom-out',
              icon: Icons.zoom_out,
              label: 'Kleiner',
              onTap: () => setState(() => zoom = (zoom - .1).clamp(.8, 1.6)),
            ),
            _command(
              key: 'message-zoom-reset',
              icon: Icons.filter_center_focus,
              label: '100 %',
              onTap: () => setState(() => zoom = 1),
            ),
            _command(
              key: 'message-zoom-in',
              icon: Icons.zoom_in,
              label: 'Größer',
              onTap: () => setState(() => zoom = (zoom + .1).clamp(.8, 1.6)),
            ),
          ],
        ),
        _MessageRibbonGroup(
          label: 'Kopfzeilen',
          children: [
            _command(
              key: 'message-toggle-details',
              icon: showDetails ? Icons.expand_less : Icons.expand_more,
              label: showDetails ? 'Weniger' : 'Details',
              onTap: () => setState(() => showDetails = !showDetails),
            ),
          ],
        ),
      ],
      _ => _messageGroups(),
    };
    return Container(
      key: const Key('classic-message-ribbon'),
      height: 83,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      padding: const EdgeInsets.fromLTRB(6, 4, 6, 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: groups,
      ),
    );
  }

  List<Widget> _messageGroups() {
    if (message.draft) {
      return [
        _MessageRibbonGroup(
          label: 'Entwurf',
          children: [
            _command(
              key: 'message-edit-draft',
              icon: Icons.edit_outlined,
              label: 'Bearbeiten',
              onTap: () => widget.onEditDraft(message),
            ),
          ],
        ),
        _organizationGroup(),
      ];
    }
    return [
      _MessageRibbonGroup(
        label: 'Antworten',
        children: [
          _command(
            key: 'message-window-reply',
            icon: Icons.reply,
            label: 'Antworten',
            onTap: () => widget.onReply(message),
          ),
          _command(
            key: 'message-window-reply-all',
            icon: Icons.reply_all,
            label: 'Allen antworten',
            onTap: () => widget.onReplyAll(message),
          ),
          _command(
            key: 'message-window-forward',
            icon: Icons.forward,
            label: 'Weiterleiten',
            onTap: () => widget.onForward(message),
          ),
        ],
      ),
      _organizationGroup(),
      _MessageRibbonGroup(
        label: 'Markierungen',
        children: [
          _command(
            key: 'message-window-toggle-read',
            icon: message.unread
                ? Icons.mark_email_read_outlined
                : Icons.mark_email_unread_outlined,
            label: message.unread ? 'Gelesen' : 'Ungelesen',
            onTap: () =>
                updateMessage(message.copyWith(unread: !message.unread)),
          ),
          _command(
            key: 'message-window-toggle-flag',
            icon: message.flagged ? Icons.flag : Icons.outlined_flag,
            label: message.flagged ? 'Markiert' : 'Markieren',
            active: message.flagged,
            onTap: () =>
                updateMessage(message.copyWith(flagged: !message.flagged)),
          ),
        ],
      ),
    ];
  }

  Widget _organizationGroup() {
    final archive = folderForRole('archive');
    final trash = folderForRole('trash');
    final destinations = widget.folders
        .where(
          (folder) =>
              folder.accountId == message.accountId &&
              folder.id != message.mailboxId,
        )
        .toList(growable: false);
    return _MessageRibbonGroup(
      label: 'Verschieben',
      children: [
        _command(
          key: 'message-window-archive',
          icon: Icons.archive_outlined,
          label: 'Archivieren',
          enabled: archive != null,
          onTap: archive == null ? null : () => moveTo(archive.id, close: true),
        ),
        PopupMenuButton<String>(
          key: const Key('message-window-move'),
          tooltip: 'In Ordner verschieben',
          enabled: destinations.isNotEmpty,
          onSelected: (mailboxId) => moveTo(mailboxId, close: true),
          itemBuilder: (_) => [
            for (final folder in destinations)
              PopupMenuItem(
                value: folder.id,
                child: Text(mailboxDisplayName(context, folder)),
              ),
          ],
          child: const _MessageCommandBody(
            icon: Icons.drive_file_move_outlined,
            label: 'Verschieben',
          ),
        ),
        _command(
          key: 'message-window-delete',
          icon: Icons.delete_outline,
          label: 'Löschen',
          enabled: trash != null,
          onTap: trash == null ? null : () => moveTo(trash.id, close: true),
        ),
      ],
    );
  }

  Widget _command({
    required String key,
    required IconData icon,
    required String label,
    required VoidCallback? onTap,
    bool enabled = true,
    bool active = false,
  }) {
    return InkWell(
      key: Key(key),
      onTap: enabled ? onTap : null,
      child: _MessageCommandBody(
        icon: icon,
        label: label,
        enabled: enabled,
        active: active,
      ),
    );
  }

  Widget _content() {
    final to = message.draft ? message.draftTo : message.toRecipients;
    final cc = message.draft ? message.draftCc : message.ccRecipients;
    final bcc = message.draft ? message.draftBcc : message.bccRecipients;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          key: const Key('classic-message-header'),
          padding: const EdgeInsets.fromLTRB(22, 17, 22, 13),
          decoration: BoxDecoration(
            border: Border(
              bottom: BorderSide(color: MaicentaPalette.of(context).border),
            ),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                message.subject.isEmpty ? '(Kein Betreff)' : message.subject,
                style: TextStyle(
                  fontSize: 21 * zoom,
                  fontWeight: FontWeight.w500,
                ),
              ),
              const SizedBox(height: 14),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  CircleAvatar(
                    radius: 20,
                    backgroundColor: MaicentaPalette.of(context).selected,
                    child: Text(
                      _initials(message.sender),
                      style: const TextStyle(
                        color: _outlookBlue,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        SelectableText(
                          '${message.sender} <${message.email}>',
                          style: const TextStyle(fontWeight: FontWeight.w600),
                        ),
                        if (showDetails) ...[
                          _addressLine('An', to),
                          if (cc.isNotEmpty) _addressLine('Cc', cc),
                          if (bcc.isNotEmpty) _addressLine('Bcc', bcc),
                        ],
                      ],
                    ),
                  ),
                  Text(
                    message.time,
                    style: TextStyle(
                      fontSize: 11,
                      color: MaicentaPalette.of(context).mutedText,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
        Container(
          height: 29,
          color: MaicentaPalette.of(context).chrome,
          padding: const EdgeInsets.symmetric(horizontal: 22),
          alignment: Alignment.centerLeft,
          child: Text(
            message.draft
                ? message.draftSynchronized
                      ? 'IMAP-Entwurf · Synchronisiert'
                      : 'Lokaler Entwurf · Synchronisierung ausstehend'
                : 'Sichere Standard-HTML-Ansicht · Externe Inhalte blockiert',
            style: const TextStyle(fontSize: 10.5),
          ),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(28, 24, 36, 32),
            child: SelectionArea(
              child: HtmlWidget(
                key: const Key('message-window-html-body'),
                message.body,
                textStyle: TextStyle(fontSize: 13.5 * zoom, height: 1.5),
                onTapUrl: (url) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(content: Text('Externer Link blockiert: $url')),
                  );
                  return true;
                },
              ),
            ),
          ),
        ),
        if (message.attachments.isNotEmpty) _attachments(),
      ],
    );
  }

  Widget _addressLine(String label, String value) {
    return Padding(
      padding: const EdgeInsets.only(top: 3),
      child: SelectableText(
        '$label: ${value.isEmpty ? 'Nicht angegeben' : value}',
        style: TextStyle(
          fontSize: 11,
          color: MaicentaPalette.of(context).mutedText,
        ),
      ),
    );
  }

  Widget _attachments() {
    return Container(
      key: const Key('message-window-attachments'),
      padding: const EdgeInsets.fromLTRB(20, 9, 20, 11),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).pane,
        border: Border(
          top: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Wrap(
        spacing: 8,
        runSpacing: 8,
        children: [
          for (final attachment in message.attachments)
            OutlinedButton.icon(
              key: Key('message-window-attachment-${attachment.id}'),
              onPressed: () => widget.onSaveAttachment(attachment),
              icon: const Icon(Icons.attach_file, size: 16),
              label: Text(
                '${attachment.fileName} · ${_formatBytes(attachment.sizeBytes)}',
              ),
            ),
        ],
      ),
    );
  }

  Widget _statusBar() {
    return Container(
      key: const Key('classic-message-status-bar'),
      height: 24,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).subtle,
        border: Border(
          top: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          Text(
            message.unread ? 'Ungelesen' : 'Gelesen',
            style: const TextStyle(fontSize: 10),
          ),
          if (message.flagged) ...[
            const SizedBox(width: 12),
            const Icon(Icons.flag, size: 13, color: Color(0xFFC74432)),
            const SizedBox(width: 3),
            const Text('Markiert', style: TextStyle(fontSize: 10)),
          ],
          const Spacer(),
          Text(
            '${(zoom * 100).round()} %',
            style: const TextStyle(fontSize: 10),
          ),
        ],
      ),
    );
  }
}

class _MessageRibbonGroup extends StatelessWidget {
  const _MessageRibbonGroup({required this.label, required this.children});

  final String label;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(right: 5),
      padding: const EdgeInsets.symmetric(horizontal: 5),
      decoration: BoxDecoration(
        border: Border(
          right: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Column(
        children: [
          Expanded(
            child: Row(mainAxisSize: MainAxisSize.min, children: children),
          ),
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

class _MessageCommandBody extends StatelessWidget {
  const _MessageCommandBody({
    required this.icon,
    required this.label,
    this.enabled = true,
    this.active = false,
  });

  final IconData icon;
  final String label;
  final bool enabled;
  final bool active;

  @override
  Widget build(BuildContext context) {
    final color = enabled
        ? Theme.of(context).colorScheme.onSurface
        : Theme.of(context).disabledColor;
    return Container(
      width: 66,
      padding: const EdgeInsets.symmetric(horizontal: 3, vertical: 2),
      color: active ? MaicentaPalette.of(context).selected : Colors.transparent,
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(icon, size: 23, color: active ? _outlookBlue : color),
          const SizedBox(height: 3),
          Text(
            label,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.center,
            style: TextStyle(fontSize: 9.5, color: color, height: 1.05),
          ),
        ],
      ),
    );
  }
}

String _initials(String value) {
  final words = value.trim().split(RegExp(r'\s+'));
  if (words.isEmpty || words.first.isEmpty) return '?';
  return words.take(2).map((word) => word[0].toUpperCase()).join();
}

String _formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}
