import 'package:flutter/material.dart';

import '../../app_theme.dart';
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

/// Outcome of the CalDAV/CardDAV account window.
class DavAccountSetupResult {
  const DavAccountSetupResult({required this.account, required this.password});

  final DavAccountConfig account;

  /// Empty when an existing account keeps its stored password.
  final String password;
}

/// Opens the window to add or edit a CalDAV/CardDAV account. `onTest`
/// contacts the server and returns the collections it offers.
Future<DavAccountSetupResult?> showDavAccountDialog(
  BuildContext context, {
  DavAccountConfig? existing,
  required Future<List<DiscoveredCollection>> Function(
    DavAccountConfig account,
    String password,
  )
  onTest,
}) {
  return showDialog<DavAccountSetupResult>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) =>
        _DavAccountDialog(existing: existing, onTest: onTest),
  );
}

class _DavAccountDialog extends StatefulWidget {
  const _DavAccountDialog({required this.existing, required this.onTest});

  final DavAccountConfig? existing;
  final Future<List<DiscoveredCollection>> Function(
    DavAccountConfig account,
    String password,
  )
  onTest;

  @override
  State<_DavAccountDialog> createState() => _DavAccountDialogState();
}

class _DavAccountDialogState extends State<_DavAccountDialog> {
  final formKey = GlobalKey<FormState>();
  late final TextEditingController name;
  late final TextEditingController serverUrl;
  late final TextEditingController username;
  final password = TextEditingController();
  bool testing = false;
  String? testError;
  List<DiscoveredCollection>? discovered;

  @override
  void initState() {
    super.initState();
    name = TextEditingController(text: widget.existing?.displayName ?? '');
    serverUrl = TextEditingController(text: widget.existing?.serverUrl ?? '');
    username = TextEditingController(text: widget.existing?.username ?? '');
  }

  @override
  void dispose() {
    for (final controller in [name, serverUrl, username, password]) {
      controller.dispose();
    }
    super.dispose();
  }

  DavAccountConfig draft() => DavAccountConfig(
    id: widget.existing?.id ?? 'dav.${DateTime.now().microsecondsSinceEpoch}',
    displayName: name.text.trim(),
    serverUrl: serverUrl.text.trim(),
    username: username.text.trim(),
    lastSyncAt: widget.existing?.lastSyncAt,
  );

  Future<void> test() async {
    if (!(formKey.currentState?.validate() ?? false)) return;
    setState(() {
      testing = true;
      testError = null;
      discovered = null;
    });
    try {
      final found = await widget.onTest(draft(), password.text);
      if (!mounted) return;
      setState(() => discovered = found);
    } on Object catch (error) {
      if (!mounted) return;
      setState(() => testError = error.toString());
    } finally {
      if (mounted) setState(() => testing = false);
    }
  }

  void save() {
    if (!(formKey.currentState?.validate() ?? false)) return;
    Navigator.pop(
      context,
      DavAccountSetupResult(account: draft(), password: password.text),
    );
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.existing != null;
    final palette = MaicentaPalette.of(context);
    return EditorDialog(
      title: editing
          ? 'Kalender- und Kontaktkonto bearbeiten'
          : 'Kalender- und Kontaktkonto hinzufügen',
      icon: Icons.cloud_sync_outlined,
      width: 560,
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Abbrechen'),
        ),
        OutlinedButton.icon(
          key: const Key('dav-test'),
          onPressed: testing ? null : test,
          icon: testing
              ? const SizedBox.square(
                  dimension: 14,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              : const Icon(Icons.network_check, size: 18),
          label: const Text('Verbindung testen'),
        ),
        FilledButton(
          key: const Key('dav-save'),
          onPressed: testing ? null : save,
          child: const Text('Speichern'),
        ),
      ],
      children: [
        Text(
          'Verbindet einen CalDAV/CardDAV-Server wie Nextcloud, iCloud, '
          'Posteo, mailbox.org, Synology oder Radicale. Kalender, '
          'Aufgabenlisten und Adressbücher des Kontos werden in beide '
          'Richtungen abgeglichen.',
          style: TextStyle(fontSize: 12, color: palette.mutedText),
        ),
        const SizedBox(height: 14),
        Form(
          key: formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextFormField(
                key: const Key('dav-name'),
                controller: name,
                autofocus: !editing,
                decoration: const InputDecoration(labelText: 'Kontoname'),
                validator: (value) => (value?.trim().isEmpty ?? true)
                    ? 'Bitte einen Namen eingeben.'
                    : null,
              ),
              const SizedBox(height: 12),
              TextFormField(
                key: const Key('dav-server'),
                controller: serverUrl,
                decoration: const InputDecoration(
                  labelText: 'Server-Adresse',
                  hintText: 'https://cloud.example.org/remote.php/dav/',
                ),
                keyboardType: TextInputType.url,
                validator: (value) {
                  final text = value?.trim() ?? '';
                  if (text.isEmpty) return 'Bitte die Server-Adresse eingeben.';
                  if (text.startsWith('http://') &&
                      !text.startsWith('http://localhost') &&
                      !text.startsWith('http://127.0.0.1')) {
                    return 'Nur https:// wird unterstützt.';
                  }
                  return null;
                },
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: TextFormField(
                      key: const Key('dav-username'),
                      controller: username,
                      decoration: const InputDecoration(
                        labelText: 'Benutzername',
                      ),
                      validator: (value) => (value?.trim().isEmpty ?? true)
                          ? 'Bitte den Benutzernamen eingeben.'
                          : null,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextFormField(
                      key: const Key('dav-password'),
                      controller: password,
                      obscureText: true,
                      decoration: InputDecoration(
                        labelText: editing
                            ? 'Passwort (leer = unverändert)'
                            : 'Passwort oder App-Passwort',
                      ),
                      validator: (value) => !editing && (value?.isEmpty ?? true)
                          ? 'Bitte das Passwort eingeben.'
                          : null,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
        if (testError case final error?)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(
              error,
              key: const Key('dav-test-error'),
              style: TextStyle(
                color: Theme.of(context).colorScheme.error,
                fontSize: 12,
              ),
            ),
          ),
        if (discovered case final found?)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  found.isEmpty
                      ? 'Verbindung erfolgreich, aber keine Kalender oder '
                            'Adressbücher gefunden.'
                      : 'Verbindung erfolgreich. Gefunden:',
                  key: const Key('dav-test-result'),
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                for (final collection in found)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Row(
                      children: [
                        Icon(
                          switch (collection.kind) {
                            CollectionKind.calendar =>
                              Icons.calendar_month_outlined,
                            CollectionKind.taskList => Icons.check_box_outlined,
                            CollectionKind.addressBook => Icons.people_outline,
                          },
                          size: 16,
                          color: palette.mutedText,
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            '${collection.name} · ${collection.kind.label}',
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}
