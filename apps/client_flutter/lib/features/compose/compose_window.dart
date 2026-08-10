import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:flutter_quill/flutter_quill.dart';

import '../../app_theme.dart';

const _outlookBlue = Color(0xFF0F6CBD);

enum ComposeDisposition { sent, draft }

class ComposeSender {
  const ComposeSender({required this.accountId, required this.label});

  final String accountId;
  final String label;
}

const _personalComposeSenders = [
  ComposeSender(
    accountId: 'personal',
    label: 'Persönliches Konto <demo@maicenta.local>',
  ),
];

class ComposeResult {
  const ComposeResult({
    required this.disposition,
    required this.accountId,
    required this.to,
    required this.cc,
    required this.bcc,
    required this.subject,
    required this.plainText,
    required this.htmlText,
    required this.attachments,
    required this.highImportance,
    required this.editorDeltaJson,
  });

  final ComposeDisposition disposition;
  final String accountId;
  final String to;
  final String cc;
  final String bcc;
  final String subject;
  final String plainText;
  final String htmlText;
  final List<ComposeAttachment> attachments;
  bool get hasAttachment => attachments.isNotEmpty;
  List<String> get attachmentPaths => attachments
      .where((attachment) => attachment.path.isNotEmpty)
      .map((attachment) => attachment.path)
      .toList(growable: false);
  List<String> get storedAttachmentIds => attachments
      .map((attachment) => attachment.storedAttachmentId)
      .whereType<String>()
      .toList(growable: false);
  final bool highImportance;
  final String editorDeltaJson;

  Future<void> releaseSecurityScopedResources() async {
    for (final attachment in attachments) {
      final bookmark = attachment.securityScopedBookmark;
      if (bookmark == null || bookmark.isEmpty) continue;
      try {
        await DesktopDrop.instance.stopAccessingSecurityScopedResource(
          bookmark: bookmark,
        );
      } on Object {
        // The operating system releases remaining scopes when the app exits.
      }
    }
  }
}

class ComposeAttachment {
  const ComposeAttachment({
    required this.path,
    required this.name,
    required this.size,
    this.storedAttachmentId,
    this.securityScopedBookmark,
  });

  final String path;
  final String name;
  final int size;
  final String? storedAttachmentId;
  final Uint8List? securityScopedBookmark;
}

Future<ComposeResult?> showComposeWindow(
  BuildContext context, {
  String initialTo = '',
  String initialSubject = '',
  String initialBody = '',
  String initialCc = '',
  String initialBcc = '',
  String initialAccountId = '',
  String initialEditorDeltaJson = '',
  List<ComposeAttachment> initialAttachments = const [],
  bool initialHighImportance = false,
  List<ComposeSender> senders = _personalComposeSenders,
}) async {
  return showDialog<ComposeResult>(
    context: context,
    barrierDismissible: false,
    builder: (_) => Dialog.fullscreen(
      child: ComposeWindow(
        initialTo: initialTo,
        initialSubject: initialSubject,
        initialBody: initialBody,
        initialCc: initialCc,
        initialBcc: initialBcc,
        initialAccountId: initialAccountId,
        initialEditorDeltaJson: initialEditorDeltaJson,
        initialAttachments: initialAttachments,
        initialHighImportance: initialHighImportance,
        senders: senders,
      ),
    ),
  );
}

class ComposeWindow extends StatefulWidget {
  const ComposeWindow({
    super.key,
    this.initialTo = '',
    this.initialSubject = '',
    this.initialBody = '',
    this.initialCc = '',
    this.initialBcc = '',
    this.initialAccountId = '',
    this.initialEditorDeltaJson = '',
    this.initialAttachments = const [],
    this.initialHighImportance = false,
    this.senders = _personalComposeSenders,
  });

  final String initialTo;
  final String initialSubject;
  final String initialBody;
  final String initialCc;
  final String initialBcc;
  final String initialAccountId;
  final String initialEditorDeltaJson;
  final List<ComposeAttachment> initialAttachments;
  final bool initialHighImportance;
  final List<ComposeSender> senders;

  @override
  State<ComposeWindow> createState() => _ComposeWindowState();
}

class _ComposeWindowState extends State<ComposeWindow> {
  late final QuillController editorController;
  final TextEditingController toController = TextEditingController();
  final TextEditingController ccController = TextEditingController();
  final TextEditingController bccController = TextEditingController();
  final TextEditingController subjectController = TextEditingController();
  final FocusNode editorFocus = FocusNode();
  final ScrollController editorScroll = ScrollController();

  bool showCc = false;
  bool showBcc = false;
  final List<ComposeAttachment> attachments = [];
  bool highImportance = false;
  String activeTab = 'Nachricht';
  late String selectedAccountId;
  String? validationMessage;
  bool draggingFiles = false;
  bool handedOffSecurityScopes = false;

  @override
  void initState() {
    super.initState();
    selectedAccountId =
        widget.senders.any(
          (sender) => sender.accountId == widget.initialAccountId,
        )
        ? widget.initialAccountId
        : widget.senders.first.accountId;
    toController.text = widget.initialTo;
    ccController.text = widget.initialCc;
    bccController.text = widget.initialBcc;
    subjectController.text = widget.initialSubject;
    showCc = widget.initialCc.isNotEmpty;
    showBcc = widget.initialBcc.isNotEmpty;
    attachments.addAll(widget.initialAttachments);
    highImportance = widget.initialHighImportance;
    editorController = QuillController(
      document: initialDocument(),
      selection: const TextSelection.collapsed(offset: 0),
    );
  }

  Document initialDocument() {
    if (widget.initialEditorDeltaJson.isNotEmpty) {
      try {
        final decoded = jsonDecode(widget.initialEditorDeltaJson);
        if (decoded is List<dynamic>) {
          return Document.fromJson(decoded);
        }
      } on Object {
        // Older drafts fall back to their persisted plain-text body.
      }
    }
    final document = Document();
    if (widget.initialBody.isNotEmpty) {
      document.insert(0, widget.initialBody);
    }
    return document;
  }

  @override
  void dispose() {
    if (!handedOffSecurityScopes) {
      unawaited(_releaseSecurityScopes(attachments));
    }
    editorController.dispose();
    toController.dispose();
    ccController.dispose();
    bccController.dispose();
    subjectController.dispose();
    editorFocus.dispose();
    editorScroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: MaicentaPalette.of(context).window,
      body: DropTarget(
        key: const Key('compose-file-drop-target'),
        onDragEntered: (_) => setState(() => draggingFiles = true),
        onDragExited: (_) => setState(() => draggingFiles = false),
        onDragDone: (details) {
          setState(() => draggingFiles = false);
          unawaited(_acceptDroppedFiles(details.files));
        },
        child: Stack(
          fit: StackFit.expand,
          children: [
            Column(
              children: [
                _ComposeTitleBar(onSaveDraft: saveDraft, onClose: closeWindow),
                _ComposeTabs(
                  activeTab: activeTab,
                  onSelected: (tab) => setState(() => activeTab = tab),
                ),
                _buildRibbon(),
                if (validationMessage != null)
                  Container(
                    width: double.infinity,
                    color: MaicentaPalette.of(context).warning,
                    padding: const EdgeInsets.symmetric(
                      horizontal: 18,
                      vertical: 7,
                    ),
                    child: Text(
                      validationMessage!,
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                _buildEnvelope(),
                if (attachments.isNotEmpty)
                  Container(
                    key: const Key('classic-compose-attachments'),
                    height: 39,
                    padding: const EdgeInsets.only(left: 18, right: 12),
                    alignment: Alignment.centerLeft,
                    decoration: BoxDecoration(
                      color: MaicentaPalette.of(context).pane,
                      border: Border(
                        bottom: BorderSide(
                          color: MaicentaPalette.of(context).border,
                        ),
                      ),
                    ),
                    child: ListView.separated(
                      scrollDirection: Axis.horizontal,
                      itemCount: attachments.length,
                      separatorBuilder: (_, _) => const SizedBox(width: 6),
                      itemBuilder: (context, index) {
                        final attachment = attachments[index];
                        return _ComposeAttachmentTile(
                          attachment: attachment,
                          onRemove: () => removeAttachment(index),
                        );
                      },
                    ),
                  ),
                Expanded(
                  child: Container(
                    key: const Key('classic-compose-editor-area'),
                    color: MaicentaPalette.of(context).window,
                    child: QuillEditor(
                      key: const Key('compose-editor'),
                      controller: editorController,
                      focusNode: editorFocus,
                      scrollController: editorScroll,
                      config: const QuillEditorConfig(
                        padding: EdgeInsets.fromLTRB(22, 20, 28, 24),
                        placeholder: 'Nachricht verfassen …',
                        expands: true,
                      ),
                    ),
                  ),
                ),
                _ComposeStatusBar(controller: editorController),
              ],
            ),
            if (draggingFiles)
              IgnorePointer(
                child: Container(
                  key: const Key('compose-file-drop-overlay'),
                  margin: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: MaicentaPalette.of(
                      context,
                    ).window.withValues(alpha: 0.94),
                    border: Border.all(color: _outlookBlue, width: 2),
                  ),
                  child: const Center(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(
                          Icons.file_download_outlined,
                          size: 48,
                          color: _outlookBlue,
                        ),
                        SizedBox(height: 12),
                        Text(
                          'Dateien hier ablegen',
                          style: TextStyle(
                            fontSize: 18,
                            fontWeight: FontWeight.w600,
                            color: _outlookBlue,
                          ),
                        ),
                        SizedBox(height: 5),
                        Text('Die Dateien werden als Anhänge hinzugefügt.'),
                      ],
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildEnvelope() {
    return Container(
      key: const Key('classic-compose-envelope'),
      color: MaicentaPalette.of(context).window,
      child: IntrinsicHeight(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              width: 112,
              child: Align(
                alignment: Alignment.topCenter,
                child: Padding(
                  padding: const EdgeInsets.only(top: 11),
                  child: FilledButton.icon(
                    key: const Key('compose-send'),
                    onPressed: sendMessage,
                    icon: const Icon(Icons.send, size: 19),
                    label: const Text(
                      'Senden',
                      key: Key('compose-send-label'),
                      maxLines: 1,
                      softWrap: false,
                    ),
                    style: FilledButton.styleFrom(
                      minimumSize: const Size(96, 54),
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      backgroundColor: _outlookBlue,
                      shape: const RoundedRectangleBorder(),
                    ),
                  ),
                ),
              ),
            ),
            const VerticalDivider(width: 1),
            Expanded(
              child: Column(
                children: [
                  _AddressRow(
                    label: 'Von',
                    child: DropdownButton<String>(
                      key: const Key('compose-from'),
                      value: selectedAccountId,
                      isDense: true,
                      isExpanded: true,
                      underline: const SizedBox.shrink(),
                      items: [
                        for (final sender in widget.senders)
                          DropdownMenuItem(
                            value: sender.accountId,
                            child: Text(
                              sender.label,
                              style: const TextStyle(fontSize: 12),
                            ),
                          ),
                      ],
                      onChanged: (value) {
                        if (value != null) {
                          setState(() => selectedAccountId = value);
                        }
                      },
                    ),
                  ),
                  _AddressRow(
                    label: 'An',
                    child: Row(
                      children: [
                        Expanded(
                          child: TextField(
                            key: const Key('compose-to'),
                            controller: toController,
                            autofocus: true,
                            decoration: const InputDecoration(
                              hintText: 'Name oder E-Mail-Adresse eingeben',
                            ),
                          ),
                        ),
                        TextButton(
                          key: const Key('compose-show-cc'),
                          onPressed: () => setState(() => showCc = !showCc),
                          child: const Text('Cc'),
                        ),
                        TextButton(
                          key: const Key('compose-show-bcc'),
                          onPressed: () => setState(() => showBcc = !showBcc),
                          child: const Text('Bcc'),
                        ),
                      ],
                    ),
                  ),
                  if (showCc)
                    _AddressRow(
                      label: 'Cc',
                      child: TextField(
                        key: const Key('compose-cc'),
                        controller: ccController,
                        decoration: const InputDecoration(
                          hintText: 'Kopieempfänger hinzufügen',
                        ),
                      ),
                    ),
                  if (showBcc)
                    _AddressRow(
                      label: 'Bcc',
                      child: TextField(
                        key: const Key('compose-bcc'),
                        controller: bccController,
                        decoration: const InputDecoration(
                          hintText: 'Blindkopieempfänger hinzufügen',
                        ),
                      ),
                    ),
                  _AddressRow(
                    label: 'Betreff',
                    child: TextField(
                      key: const Key('compose-subject'),
                      controller: subjectController,
                      decoration: const InputDecoration(
                        hintText: 'Betreff hinzufügen',
                      ),
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

  Widget _buildRibbon() {
    late final Widget content;
    if (activeTab == 'Datei') {
      content = Row(
        children: [
          _ComposeRibbonGroup(
            label: 'Entwurf',
            child: Row(
              children: [
                _ComposeCommand(
                  key: const Key('compose-ribbon-save-draft'),
                  icon: Icons.save_outlined,
                  label: 'Speichern',
                  onTap: saveDraft,
                ),
                _ComposeCommand(
                  key: const Key('compose-file-close'),
                  icon: Icons.close,
                  label: 'Schließen',
                  onTap: closeWindow,
                ),
              ],
            ),
          ),
        ],
      );
    } else if (activeTab == 'Optionen') {
      content = Row(
        children: [
          _ComposeRibbonGroup(
            label: 'Felder anzeigen',
            child: Row(
              children: [
                _ComposeCommand(
                  key: const Key('compose-ribbon-cc'),
                  icon: Icons.group_outlined,
                  label: 'Cc',
                  active: showCc,
                  onTap: () => setState(() => showCc = !showCc),
                ),
                _ComposeCommand(
                  key: const Key('compose-ribbon-bcc'),
                  icon: Icons.visibility_off_outlined,
                  label: 'Bcc',
                  active: showBcc,
                  onTap: () => setState(() => showBcc = !showBcc),
                ),
              ],
            ),
          ),
          _ComposeRibbonGroup(
            label: 'Nachverfolgung',
            child: _importanceCommand(),
          ),
        ],
      );
    } else if (activeTab == 'Einfügen') {
      content = Row(
        children: [
          _ComposeRibbonGroup(
            label: 'Einschließen',
            child: Row(
              children: [
                _ComposeCommand(
                  key: const Key('compose-attach'),
                  icon: Icons.attach_file,
                  label: 'Datei anfügen',
                  onTap: () => attachFiles(),
                ),
                _ComposeCommand(
                  key: const Key('compose-signature'),
                  icon: Icons.badge_outlined,
                  label: 'Signatur',
                  onTap: insertSignature,
                ),
              ],
            ),
          ),
          Expanded(
            child: _ComposeRibbonGroup(
              label: 'Text und Links',
              child: _formattingToolbar(),
            ),
          ),
        ],
      );
    } else if (activeTab == 'Text formatieren') {
      content = Row(
        children: [
          Expanded(
            child: _ComposeRibbonGroup(
              label: 'Schriftart und Absatz',
              child: _formattingToolbar(),
            ),
          ),
        ],
      );
    } else {
      content = Row(
        children: [
          _ComposeRibbonGroup(
            label: 'Einschließen',
            child: Row(
              children: [
                _ComposeCommand(
                  key: const Key('compose-attach'),
                  icon: Icons.attach_file,
                  label: 'Datei anfügen',
                  onTap: () => attachFiles(),
                ),
                _ComposeCommand(
                  key: const Key('compose-signature'),
                  icon: Icons.badge_outlined,
                  label: 'Signatur',
                  onTap: insertSignature,
                ),
              ],
            ),
          ),
          _ComposeRibbonGroup(label: 'Kategorien', child: _importanceCommand()),
          Expanded(
            child: _ComposeRibbonGroup(
              label: 'Text',
              child: _formattingToolbar(),
            ),
          ),
        ],
      );
    }
    return Container(
      key: const Key('classic-compose-ribbon'),
      height: 86,
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(6, 3, 6, 2),
        child: content,
      ),
    );
  }

  Widget _importanceCommand() {
    return _ComposeCommand(
      key: const Key('compose-importance'),
      icon: Icons.priority_high,
      label: highImportance ? 'Hohe Priorität' : 'Wichtigkeit',
      active: highImportance,
      onTap: () => setState(() {
        highImportance = !highImportance;
        validationMessage = highImportance
            ? 'Die Nachricht ist als wichtig markiert.'
            : null;
      }),
    );
  }

  Widget _formattingToolbar() {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 4),
      child: QuillSimpleToolbar(
        controller: editorController,
        config: QuillSimpleToolbarConfig(
          multiRowsDisplay: false,
          showFontFamily: true,
          showFontSize: true,
          showBoldButton: true,
          showItalicButton: true,
          showUnderLineButton: true,
          showStrikeThrough: true,
          showColorButton: true,
          showBackgroundColorButton: true,
          showAlignmentButtons: true,
          showListNumbers: true,
          showListBullets: true,
          showIndent: true,
          showLink: true,
          showUndo: true,
          showRedo: true,
          showClearFormat: true,
          showHeaderStyle: false,
          showInlineCode: false,
          showCodeBlock: false,
          showQuote: false,
          showListCheck: false,
          showSearchButton: false,
          showSubscript: false,
          showSuperscript: false,
          decoration: BoxDecoration(color: MaicentaPalette.of(context).chrome),
        ),
      ),
    );
  }

  void insertSignature() {
    final selection = editorController.selection;
    final insertionPoint = selection.isValid
        ? selection.extentOffset
        : editorController.document.length - 1;
    const signature = '\n\nViele Grüße\nTimotheus\n';
    editorController.replaceText(
      insertionPoint,
      0,
      signature,
      TextSelection.collapsed(offset: insertionPoint + signature.length),
    );
    editorFocus.requestFocus();
  }

  Future<void> attachFiles() async {
    late final List<XFile> selected;
    try {
      selected = await openFiles(confirmButtonText: 'Anfügen');
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => validationMessage = 'Dateiauswahl fehlgeschlagen: $error');
      return;
    }
    if (!mounted || selected.isEmpty) return;
    await _addAttachmentFiles(selected);
  }

  Future<void> _acceptDroppedFiles(List<DropItem> droppedItems) async {
    final files = droppedItems.whereType<DropItemFile>().toList();
    if (files.length != droppedItems.length && mounted) {
      setState(
        () => validationMessage =
            'Ordner können nicht als Anhang abgelegt werden.',
      );
    }
    final startedScopes = <String, Uint8List>{};
    for (final file in files) {
      final bookmark = file.extraAppleBookmark;
      if (bookmark == null || bookmark.isEmpty) continue;
      try {
        final started = await DesktopDrop.instance
            .startAccessingSecurityScopedResource(bookmark: bookmark);
        if (started) startedScopes[file.path] = bookmark;
      } on Object {
        // Non-macOS platforms do not need a security-scoped resource.
      }
    }
    final accepted = await _addAttachmentFiles(
      files,
      securityScopes: startedScopes,
    );
    for (final entry in startedScopes.entries) {
      if (accepted.contains(entry.key)) continue;
      try {
        await DesktopDrop.instance.stopAccessingSecurityScopedResource(
          bookmark: entry.value,
        );
      } on Object {
        // Best-effort cleanup; the operating system also releases it on exit.
      }
    }
  }

  Future<Set<String>> _addAttachmentFiles(
    Iterable<XFile> selected, {
    Map<String, Uint8List> securityScopes = const {},
  }) async {
    final additions = <ComposeAttachment>[];
    final acceptedPaths = <String>{};
    var totalSize = attachments.fold<int>(0, (sum, item) => sum + item.size);
    for (final file in selected) {
      if (attachments.any((attachment) => attachment.path == file.path) ||
          additions.any((attachment) => attachment.path == file.path)) {
        continue;
      }
      if (attachments.length + additions.length >= 10) {
        setState(
          () => validationMessage =
              'Es können höchstens 10 Dateien angefügt werden.',
        );
        break;
      }
      late final int size;
      try {
        size = await file.length();
      } on Object catch (error) {
        if (!mounted) return acceptedPaths;
        setState(
          () => validationMessage =
              '„${file.name}“ konnte nicht gelesen werden: $error',
        );
        continue;
      }
      if (totalSize + size > 18 * 1024 * 1024) {
        if (!mounted) return acceptedPaths;
        setState(
          () => validationMessage =
              'Die Anhänge dürfen zusammen höchstens 18 MiB groß sein.',
        );
        break;
      }
      additions.add(
        ComposeAttachment(
          path: file.path,
          name: file.name,
          size: size,
          securityScopedBookmark: securityScopes[file.path],
        ),
      );
      acceptedPaths.add(file.path);
      totalSize += size;
    }
    if (!mounted || additions.isEmpty) return acceptedPaths;
    setState(() {
      attachments.addAll(additions);
      validationMessage = null;
    });
    return acceptedPaths;
  }

  void removeAttachment(int index) {
    final attachment = attachments.removeAt(index);
    setState(() {});
    unawaited(_releaseSecurityScopes([attachment]));
  }

  Future<void> _releaseSecurityScopes(Iterable<ComposeAttachment> items) async {
    for (final attachment in items) {
      final bookmark = attachment.securityScopedBookmark;
      if (bookmark == null || bookmark.isEmpty) continue;
      try {
        await DesktopDrop.instance.stopAccessingSecurityScopedResource(
          bookmark: bookmark,
        );
      } on Object {
        // Best-effort cleanup; the operating system also releases it on exit.
      }
    }
  }

  void saveDraft() {
    Navigator.pop(context, composeResult(ComposeDisposition.draft));
  }

  void sendMessage() {
    if (toController.text.trim().isEmpty) {
      setState(
        () => validationMessage = 'Bitte mindestens einen Empfänger angeben.',
      );
      return;
    }
    if (subjectController.text.trim().isEmpty) {
      setState(() => validationMessage = 'Bitte einen Betreff angeben.');
      return;
    }
    Navigator.pop(context, composeResult(ComposeDisposition.sent));
  }

  ComposeResult composeResult(ComposeDisposition disposition) {
    handedOffSecurityScopes = true;
    return ComposeResult(
      disposition: disposition,
      accountId: selectedAccountId,
      to: toController.text.trim(),
      cc: ccController.text.trim(),
      bcc: bccController.text.trim(),
      subject: subjectController.text.trim().isEmpty
          ? '(Kein Betreff)'
          : subjectController.text.trim(),
      plainText: editorController.document.toPlainText().trim(),
      htmlText: quillDeltaToEmailHtml(
        editorController.document.toDelta().toJson(),
      ),
      attachments: List.unmodifiable(attachments),
      highImportance: highImportance,
      editorDeltaJson: jsonEncode(editorController.document.toDelta().toJson()),
    );
  }

  void closeWindow() {
    if (editorController.document.toPlainText().trim().isEmpty &&
        toController.text.trim().isEmpty &&
        subjectController.text.trim().isEmpty) {
      Navigator.pop(context);
      return;
    }
    showDialog<void>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Entwurf speichern?'),
        content: const Text(
          'Möchtest du diese Nachricht als Entwurf behalten oder verwerfen?',
        ),
        actions: [
          TextButton(
            onPressed: () {
              Navigator.pop(dialogContext);
              Navigator.pop(context);
            },
            child: const Text('Verwerfen'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(dialogContext);
              Navigator.pop(context, composeResult(ComposeDisposition.draft));
            },
            child: const Text('Entwurf speichern'),
          ),
        ],
      ),
    );
  }
}

class _ComposeTitleBar extends StatelessWidget {
  const _ComposeTitleBar({required this.onSaveDraft, required this.onClose});

  final VoidCallback onSaveDraft;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return Container(
      key: const Key('classic-compose-title-bar'),
      height: 39,
      padding: const EdgeInsets.only(left: 8),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).chrome,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          const Icon(Icons.mail_outline, color: _outlookBlue, size: 17),
          IconButton(
            key: const Key('compose-quick-save-draft'),
            tooltip: 'Entwurf speichern',
            visualDensity: VisualDensity.compact,
            onPressed: onSaveDraft,
            icon: Icon(
              Icons.save_outlined,
              color: Theme.of(context).colorScheme.onSurface,
              size: 17,
            ),
          ),
          const SizedBox(width: 5),
          Text(
            'Neue Nachricht · HTML',
            style: TextStyle(
              color: Theme.of(context).colorScheme.onSurface,
              fontSize: 12,
              fontWeight: FontWeight.w500,
            ),
          ),
          const Spacer(),
          IconButton(
            key: const Key('compose-close'),
            tooltip: 'Fenster schließen',
            onPressed: onClose,
            icon: Icon(
              Icons.close,
              color: Theme.of(context).colorScheme.onSurface,
              size: 18,
            ),
          ),
        ],
      ),
    );
  }
}

class _ComposeTabs extends StatelessWidget {
  const _ComposeTabs({required this.activeTab, required this.onSelected});

  final String activeTab;
  final ValueChanged<String> onSelected;

  @override
  Widget build(BuildContext context) {
    const tabs = [
      'Datei',
      'Nachricht',
      'Text formatieren',
      'Einfügen',
      'Optionen',
    ];
    return Container(
      key: const Key('classic-compose-tabs'),
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
              key: Key('compose-tab-$tab'),
              onTap: () => onSelected(tab),
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 14),
                alignment: Alignment.center,
                decoration: BoxDecoration(
                  border: Border(
                    bottom: BorderSide(
                      color: activeTab == tab
                          ? _outlookBlue
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
                        ? _outlookBlue
                        : Theme.of(context).colorScheme.onSurface,
                    fontWeight: activeTab == tab
                        ? FontWeight.w600
                        : FontWeight.normal,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _AddressRow extends StatelessWidget {
  const _AddressRow({required this.label, required this.child});

  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Container(
      constraints: const BoxConstraints(minHeight: 35),
      padding: const EdgeInsets.only(left: 11, right: 9),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).window,
        border: Border(
          bottom: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Row(
        children: [
          SizedBox(
            width: 61,
            child: Text(
              label,
              style: TextStyle(
                fontSize: 12,
                color: MaicentaPalette.of(context).mutedText,
              ),
            ),
          ),
          Expanded(child: child),
        ],
      ),
    );
  }
}

class _ComposeRibbonGroup extends StatelessWidget {
  const _ComposeRibbonGroup({required this.label, required this.child});

  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 78,
      padding: const EdgeInsets.symmetric(horizontal: 3),
      decoration: BoxDecoration(
        border: Border(
          right: BorderSide(color: MaicentaPalette.of(context).border),
        ),
      ),
      child: Column(
        children: [
          Expanded(child: child),
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

class _ComposeAttachmentTile extends StatelessWidget {
  const _ComposeAttachmentTile({
    required this.attachment,
    required this.onRemove,
  });

  final ComposeAttachment attachment;
  final VoidCallback onRemove;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 28,
      padding: const EdgeInsets.only(left: 8),
      decoration: BoxDecoration(
        color: MaicentaPalette.of(context).window,
        border: Border.all(color: MaicentaPalette.of(context).border),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const Icon(
            Icons.insert_drive_file_outlined,
            size: 15,
            color: _outlookBlue,
          ),
          const SizedBox(width: 6),
          Text(
            '${attachment.name} · ${formatFileSize(attachment.size)}',
            style: const TextStyle(fontSize: 10.5),
          ),
          IconButton(
            tooltip: 'Anhang entfernen',
            visualDensity: VisualDensity.compact,
            onPressed: onRemove,
            icon: const Icon(Icons.close, size: 14),
          ),
        ],
      ),
    );
  }
}

class _ComposeCommand extends StatelessWidget {
  const _ComposeCommand({
    super.key,
    required this.icon,
    required this.label,
    required this.onTap,
    this.active = false,
  });

  final IconData icon;
  final String label;
  final VoidCallback onTap;
  final bool active;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      child: Container(
        width: 72,
        color: active
            ? MaicentaPalette.of(context).warning
            : Colors.transparent,
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Icon(
              icon,
              size: 20,
              color: active
                  ? const Color(0xFFC74432)
                  : Theme.of(context).colorScheme.onSurface,
            ),
            const SizedBox(height: 3),
            Text(
              label,
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 10),
            ),
          ],
        ),
      ),
    );
  }
}

class _ComposeStatusBar extends StatelessWidget {
  const _ComposeStatusBar({required this.controller});

  final QuillController controller;

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: controller,
      builder: (context, _) => Container(
        key: const Key('classic-compose-status-bar'),
        height: 23,
        padding: const EdgeInsets.symmetric(horizontal: 9),
        decoration: BoxDecoration(
          color: MaicentaPalette.of(context).subtle,
          border: Border(
            top: BorderSide(color: MaicentaPalette.of(context).border),
          ),
        ),
        child: Row(
          children: [
            const Text('HTML-E-Mail', style: TextStyle(fontSize: 10)),
            const SizedBox(width: 18),
            Text(
              '${controller.document.toPlainText().trim().split(RegExp(r'\s+')).where((word) => word.isNotEmpty).length} Wörter',
              style: const TextStyle(fontSize: 10),
            ),
            const SizedBox(width: 18),
            const Text('Entwurf lokal', style: TextStyle(fontSize: 10)),
            const Spacer(),
            const Icon(Icons.shield_outlined, size: 13),
            const SizedBox(width: 4),
            const Text(
              'Keine externen Inhalte',
              style: TextStyle(fontSize: 10),
            ),
          ],
        ),
      ),
    );
  }
}

String formatFileSize(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}

/// Converts the subset of Quill Delta supported by the visible composer
/// toolbar into conservative email HTML. The Rust core sanitizes this result
/// again before local persistence or SMTP delivery.
String quillDeltaToEmailHtml(List<Map<String, dynamic>> operations) {
  final output = StringBuffer();
  final line = StringBuffer();

  for (final operation in operations) {
    final inserted = operation['insert'];
    final attributes = _deltaAttributes(operation['attributes']);
    if (inserted is! String) {
      line.write('<span>[Nicht unterstützter Inhalt]</span>');
      continue;
    }
    final segments = inserted.split('\n');
    for (var index = 0; index < segments.length; index += 1) {
      final segment = segments[index];
      if (segment.isNotEmpty) {
        line.write(_formatInlineEmailHtml(segment, attributes));
      }
      if (index < segments.length - 1) {
        _writeEmailLine(output, line.toString(), attributes);
        line.clear();
      }
    }
  }
  if (line.isNotEmpty) {
    _writeEmailLine(output, line.toString(), const {});
  }

  return '<!doctype html><html><head><meta charset="utf-8">'
      '<meta name="viewport" content="width=device-width,initial-scale=1">'
      '</head><body style="margin:0;padding:16px;font-family:Arial,sans-serif;'
      'font-size:14px;line-height:1.5;color:#242424">$output</body></html>';
}

Map<String, dynamic> _deltaAttributes(Object? value) {
  if (value is! Map) return const {};
  return value.map((key, value) => MapEntry(key.toString(), value));
}

String _formatInlineEmailHtml(String text, Map<String, dynamic> attributes) {
  var content = _escapeHtml(text);
  final styles = <String>[];
  final color = attributes['color']?.toString();
  final background = attributes['background']?.toString();
  final font = attributes['font']?.toString();
  final size = attributes['size']?.toString();
  if (_safeCssColor(color)) styles.add('color:$color');
  if (_safeCssColor(background)) styles.add('background-color:$background');
  if (font != null && RegExp(r'^[A-Za-z0-9 _,-]{1,80}$').hasMatch(font)) {
    styles.add('font-family:$font');
  }
  final fontSize = switch (size) {
    'small' => '11px',
    'large' => '18px',
    'huge' => '26px',
    _ => null,
  };
  if (fontSize != null) styles.add('font-size:$fontSize');
  if (styles.isNotEmpty) {
    content = '<span style="${styles.join(';')}">$content</span>';
  }
  if (attributes['bold'] == true) content = '<strong>$content</strong>';
  if (attributes['italic'] == true) content = '<em>$content</em>';
  if (attributes['underline'] == true) content = '<u>$content</u>';
  if (attributes['strike'] == true) content = '<s>$content</s>';

  final link = attributes['link']?.toString();
  if (_safeLink(link)) {
    content = '<a href="${_escapeHtmlAttribute(link!)}">$content</a>';
  }
  return content;
}

void _writeEmailLine(
  StringBuffer output,
  String content,
  Map<String, dynamic> attributes,
) {
  final styles = <String>['margin:0 0 8px', 'white-space:pre-wrap'];
  final alignment = attributes['align']?.toString();
  if (const {'center', 'right', 'justify'}.contains(alignment)) {
    styles.add('text-align:$alignment');
  }
  final indent = int.tryParse(attributes['indent']?.toString() ?? '') ?? 0;
  if (indent > 0) styles.add('margin-left:${indent.clamp(1, 8) * 30}px');
  final safeContent = content.isEmpty ? '<br>' : content;
  final style = styles.join(';');
  switch (attributes['list']?.toString()) {
    case 'ordered':
      output.write(
        '<ol style="margin:0 0 8px"><li style="$style">$safeContent</li></ol>',
      );
    case 'bullet':
      output.write(
        '<ul style="margin:0 0 8px"><li style="$style">$safeContent</li></ul>',
      );
    default:
      output.write('<p style="$style">$safeContent</p>');
  }
}

bool _safeCssColor(String? value) {
  if (value == null) return false;
  return RegExp(r'^(#[0-9A-Fa-f]{3,8}|rgba?\([0-9., %]+\))$').hasMatch(value);
}

bool _safeLink(String? value) {
  if (value == null) return false;
  final uri = Uri.tryParse(value);
  return uri != null && const {'http', 'https', 'mailto'}.contains(uri.scheme);
}

String _escapeHtml(String value) => value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');

String _escapeHtmlAttribute(String value) => _escapeHtml(value);
