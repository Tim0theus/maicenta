import 'package:flutter/material.dart';

import 'mail_data.dart';

final _plausibleAddress = RegExp(r'^[^\s@<>]+@[^\s@<>]+\.[^\s@<>]+$');

/// Edits the send-as aliases of one account.
///
/// Returns the new alias list when saved, or `null` when cancelled. The
/// provider remains the authority on which aliases it accepts; this dialog
/// only prevents malformed or duplicate entries.
class SenderAliasesDialog extends StatefulWidget {
  const SenderAliasesDialog({super.key, required this.account});

  final MailAccountConfig account;

  @override
  State<SenderAliasesDialog> createState() => _SenderAliasesDialogState();
}

class _AliasRow {
  _AliasRow(MailSenderAlias? alias)
    : address = TextEditingController(text: alias?.address ?? ''),
      name = TextEditingController(text: alias?.displayName ?? '');

  final TextEditingController address;
  final TextEditingController name;

  void dispose() {
    address.dispose();
    name.dispose();
  }
}

class _SenderAliasesDialogState extends State<SenderAliasesDialog> {
  late final List<_AliasRow> rows = [
    for (final alias in widget.account.aliases) _AliasRow(alias),
  ];
  String? error;

  @override
  void initState() {
    super.initState();
    if (rows.isEmpty) rows.add(_AliasRow(null));
  }

  @override
  void dispose() {
    for (final row in rows) {
      row.dispose();
    }
    super.dispose();
  }

  void addRow() => setState(() => rows.add(_AliasRow(null)));

  void removeRow(int index) {
    setState(() {
      rows.removeAt(index).dispose();
      error = null;
    });
  }

  void save() {
    final primary = widget.account.email.trim().toLowerCase();
    final seen = <String>{};
    final aliases = <MailSenderAlias>[];
    for (final row in rows) {
      final address = row.address.text.trim();
      if (address.isEmpty) continue;
      final normalized = address.toLowerCase();
      if (!_plausibleAddress.hasMatch(address)) {
        setState(() => error = '„$address“ ist keine gültige E-Mail-Adresse.');
        return;
      }
      if (normalized == primary) {
        setState(
          () =>
              error = '„$address“ ist bereits die Hauptadresse dieses Kontos.',
        );
        return;
      }
      if (!seen.add(normalized)) {
        setState(() => error = '„$address“ ist doppelt eingetragen.');
        return;
      }
      final name = row.name.text.trim();
      aliases.add(
        MailSenderAlias(
          address: address,
          displayName: name.isEmpty ? null : name,
        ),
      );
    }
    Navigator.pop(context, aliases);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return AlertDialog(
      title: Text('Absenderadressen · ${widget.account.displayName}'),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'Trage Aliase ein, die dein Anbieter für dieses Postfach '
                'freigegeben hat, zum Beispiel bei Strato, IONOS oder '
                'mailbox.org. Beim Schreiben wählst du sie im Feld „Von“. '
                'Ob eine Adresse wirklich senden darf, entscheidet der '
                'Mailserver.',
                style: theme.textTheme.bodySmall,
              ),
              const SizedBox(height: 16),
              ListTile(
                contentPadding: EdgeInsets.zero,
                dense: true,
                leading: const Icon(Icons.verified_user_outlined),
                title: Text(widget.account.email),
                subtitle: Text('Hauptadresse · ${widget.account.displayName}'),
              ),
              const Divider(),
              for (var index = 0; index < rows.length; index++)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        flex: 3,
                        child: TextField(
                          key: Key('alias-address-$index'),
                          controller: rows[index].address,
                          keyboardType: TextInputType.emailAddress,
                          decoration: const InputDecoration(
                            labelText: 'Alias-Adresse',
                            hintText: 'kontakt@beispiel.de',
                            isDense: true,
                          ),
                          onChanged: (_) {
                            if (error != null) setState(() => error = null);
                          },
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        flex: 2,
                        child: TextField(
                          key: Key('alias-name-$index'),
                          controller: rows[index].name,
                          decoration: InputDecoration(
                            labelText: 'Anzeigename (optional)',
                            hintText: widget.account.displayName,
                            isDense: true,
                          ),
                        ),
                      ),
                      IconButton(
                        key: Key('alias-remove-$index'),
                        tooltip: 'Entfernen',
                        onPressed: () => removeRow(index),
                        icon: const Icon(Icons.delete_outline),
                      ),
                    ],
                  ),
                ),
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(
                  key: const Key('alias-add'),
                  onPressed: addRow,
                  icon: const Icon(Icons.add),
                  label: const Text('Adresse hinzufügen'),
                ),
              ),
              if (error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    error!,
                    key: const Key('alias-error'),
                    style: TextStyle(color: theme.colorScheme.error),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          key: const Key('alias-save'),
          onPressed: save,
          child: const Text('Speichern'),
        ),
      ],
    );
  }
}
