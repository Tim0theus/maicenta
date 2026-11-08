import 'package:flutter/material.dart';

import '../calendar/event_editor.dart' show EditorAction;
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

class ContactEditorResult {
  const ContactEditorResult(this.action, this.contact);

  final EditorAction action;
  final LocalContactItem contact;
}

/// Opens the contact window for a new or existing contact.
Future<ContactEditorResult?> showContactEditor(
  BuildContext context, {
  LocalContactItem? contact,
  List<WorkspaceCollection> addressBooks = const [],
}) {
  return showDialog<ContactEditorResult>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) =>
        _ContactEditor(contact: contact, addressBooks: addressBooks),
  );
}

class _LabeledField {
  _LabeledField(this.label, String text)
    : controller = TextEditingController(text: text);

  ContactLabel label;
  final TextEditingController controller;

  void dispose() => controller.dispose();
}

class _AddressFields {
  _AddressFields(this.label, ContactAddress? address)
    : street = TextEditingController(text: address?.street ?? ''),
      postalCode = TextEditingController(text: address?.postalCode ?? ''),
      city = TextEditingController(text: address?.city ?? ''),
      country = TextEditingController(text: address?.country ?? '');

  ContactLabel label;
  final TextEditingController street;
  final TextEditingController postalCode;
  final TextEditingController city;
  final TextEditingController country;

  ContactAddress toAddress() => ContactAddress(
    label: label,
    street: street.text.trim(),
    postalCode: postalCode.text.trim(),
    city: city.text.trim(),
    country: country.text.trim(),
  );

  void dispose() {
    for (final controller in [street, postalCode, city, country]) {
      controller.dispose();
    }
  }
}

class _ContactEditor extends StatefulWidget {
  const _ContactEditor({required this.contact, required this.addressBooks});

  final LocalContactItem? contact;

  /// Synchronized address books the contact may be stored in.
  final List<WorkspaceCollection> addressBooks;

  @override
  State<_ContactEditor> createState() => _ContactEditorState();
}

class _ContactEditorState extends State<_ContactEditor> {
  final formKey = GlobalKey<FormState>();
  late final TextEditingController name;
  late final TextEditingController company;
  late final TextEditingController jobTitle;
  late final TextEditingController birthday;
  late final TextEditingController website;
  late final TextEditingController notes;
  late final List<_LabeledField> emails;
  late final List<_LabeledField> phones;
  late final List<_AddressFields> addresses;
  String? collectionId;

  @override
  void initState() {
    super.initState();
    final contact = widget.contact;
    collectionId = contact?.collectionId;
    if (collectionId != null &&
        !widget.addressBooks.any((book) => book.id == collectionId)) {
      collectionId = null;
    }
    name = TextEditingController(text: contact?.name ?? '');
    company = TextEditingController(text: contact?.company ?? '');
    jobTitle = TextEditingController(text: contact?.jobTitle ?? '');
    birthday = TextEditingController(
      text: contact?.birthday == null ? '' : formatDate(contact!.birthday!),
    );
    website = TextEditingController(text: contact?.website ?? '');
    notes = TextEditingController(text: contact?.notes ?? '');
    emails = [
      for (final email in contact?.emails ?? const <ContactEmail>[])
        _LabeledField(email.label, email.address),
    ];
    if (emails.isEmpty) emails.add(_LabeledField(ContactLabel.work, ''));
    phones = [
      for (final phone in contact?.phones ?? const <ContactPhone>[])
        _LabeledField(phone.label, phone.number),
    ];
    if (phones.isEmpty) phones.add(_LabeledField(ContactLabel.mobile, ''));
    addresses = [
      for (final address in contact?.addresses ?? const <ContactAddress>[])
        _AddressFields(address.label, address),
    ];
  }

  @override
  void dispose() {
    for (final controller in [
      name,
      company,
      jobTitle,
      birthday,
      website,
      notes,
    ]) {
      controller.dispose();
    }
    for (final field in [...emails, ...phones]) {
      field.dispose();
    }
    for (final address in addresses) {
      address.dispose();
    }
    super.dispose();
  }

  String? optional(TextEditingController controller) =>
      controller.text.trim().isEmpty ? null : controller.text.trim();

  void save() {
    if (!(formKey.currentState?.validate() ?? false)) return;
    final original = widget.contact;
    final contact = LocalContactItem(
      id:
          original?.id ??
          'local.contact.${DateTime.now().microsecondsSinceEpoch}',
      name: name.text.trim(),
      company: optional(company),
      jobTitle: optional(jobTitle),
      emails: [
        for (final field in emails)
          if (field.controller.text.trim().isNotEmpty)
            ContactEmail(
              label: field.label,
              address: field.controller.text.trim(),
            ),
      ],
      phones: [
        for (final field in phones)
          if (field.controller.text.trim().isNotEmpty)
            ContactPhone(
              label: field.label,
              number: field.controller.text.trim(),
            ),
      ],
      addresses: [
        for (final address in addresses)
          if (!address.toAddress().isEmpty) address.toAddress(),
      ],
      birthday: parseGermanDate(birthday.text),
      website: optional(website),
      notes: optional(notes),
      collectionId: collectionId,
    );
    Navigator.pop(context, ContactEditorResult(EditorAction.save, contact));
  }

  Future<void> delete() async {
    final contact = widget.contact;
    if (contact == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Kontakt löschen?'),
        content: Text('„${contact.name}“ wird endgültig entfernt.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            key: const Key('contact-delete-confirm'),
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Löschen'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    Navigator.pop(context, ContactEditorResult(EditorAction.delete, contact));
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.contact != null;
    return EditorDialog(
      title: editing ? 'Kontakt bearbeiten' : 'Neuer Kontakt',
      icon: Icons.person_outline,
      width: 680,
      leading: [
        if (editing)
          TextButton.icon(
            key: const Key('contact-editor-delete'),
            onPressed: delete,
            icon: const Icon(Icons.delete_outline, size: 18),
            label: const Text('Löschen'),
          ),
      ],
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          key: const Key('contact-save'),
          onPressed: save,
          child: const Text('Speichern'),
        ),
      ],
      children: [
        Form(
          key: formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextFormField(
                key: const Key('contact-name'),
                controller: name,
                autofocus: !editing,
                decoration: const InputDecoration(labelText: 'Name'),
                textInputAction: TextInputAction.next,
                validator: (value) => (value?.trim().isEmpty ?? true)
                    ? 'Bitte einen Namen eingeben.'
                    : null,
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: TextFormField(
                      key: const Key('contact-company'),
                      controller: company,
                      decoration: const InputDecoration(labelText: 'Firma'),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextFormField(
                      key: const Key('contact-job-title'),
                      controller: jobTitle,
                      decoration: const InputDecoration(labelText: 'Position'),
                    ),
                  ),
                ],
              ),
              if (widget.addressBooks.isNotEmpty) ...[
                const SizedBox(height: 12),
                DropdownButtonFormField<String?>(
                  key: const Key('contact-address-book'),
                  isExpanded: true,
                  initialValue: collectionId,
                  decoration: const InputDecoration(
                    labelText: 'Adressbuch',
                    prefixIcon: Icon(Icons.menu_book_outlined, size: 18),
                  ),
                  items: [
                    const DropdownMenuItem<String?>(
                      value: null,
                      child: Text('Meine Kontakte (lokal)'),
                    ),
                    for (final book in widget.addressBooks)
                      DropdownMenuItem<String?>(
                        value: book.id,
                        child: Text(book.name, overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: (value) => setState(() => collectionId = value),
                ),
              ],
              _groupTitle(context, 'E-Mail-Adressen'),
              for (var index = 0; index < emails.length; index++)
                _labeledRow(
                  field: emails[index],
                  labels: ContactLabel.emailLabels,
                  fieldKey: Key('contact-email-$index'),
                  hint: 'name@example.org',
                  keyboardType: TextInputType.emailAddress,
                  validator: (value) {
                    final text = value?.trim() ?? '';
                    if (text.isEmpty) return null;
                    final parts = text.split('@');
                    if (parts.length != 2 ||
                        parts[0].isEmpty ||
                        parts[1].isEmpty ||
                        text.contains(RegExp(r'\s'))) {
                      return 'Ungültige E-Mail-Adresse';
                    }
                    return null;
                  },
                  onRemove: () => setState(() {
                    emails.removeAt(index).dispose();
                    if (emails.isEmpty) {
                      emails.add(_LabeledField(ContactLabel.work, ''));
                    }
                  }),
                ),
              _addButton(
                key: const Key('contact-add-email'),
                label: 'E-Mail-Adresse hinzufügen',
                onTap: () => setState(
                  () => emails.add(_LabeledField(ContactLabel.other, '')),
                ),
              ),
              _groupTitle(context, 'Telefonnummern'),
              for (var index = 0; index < phones.length; index++)
                _labeledRow(
                  field: phones[index],
                  labels: ContactLabel.phoneLabels,
                  fieldKey: Key('contact-phone-$index'),
                  hint: '+49 …',
                  keyboardType: TextInputType.phone,
                  onRemove: () => setState(() {
                    phones.removeAt(index).dispose();
                    if (phones.isEmpty) {
                      phones.add(_LabeledField(ContactLabel.mobile, ''));
                    }
                  }),
                ),
              _addButton(
                key: const Key('contact-add-phone'),
                label: 'Telefonnummer hinzufügen',
                onTap: () => setState(
                  () => phones.add(_LabeledField(ContactLabel.work, '')),
                ),
              ),
              _groupTitle(context, 'Adressen'),
              for (var index = 0; index < addresses.length; index++)
                _addressBlock(index),
              _addButton(
                key: const Key('contact-add-address'),
                label: 'Adresse hinzufügen',
                onTap: () => setState(
                  () => addresses.add(
                    _AddressFields(
                      addresses.isEmpty ? ContactLabel.work : ContactLabel.home,
                      null,
                    ),
                  ),
                ),
              ),
              _groupTitle(context, 'Weiteres'),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: DateField(
                      fieldKey: const Key('contact-birthday'),
                      controller: birthday,
                      label: 'Geburtstag',
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextFormField(
                      key: const Key('contact-website'),
                      controller: website,
                      decoration: const InputDecoration(
                        labelText: 'Webseite',
                        hintText: 'https://',
                      ),
                      keyboardType: TextInputType.url,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              TextFormField(
                key: const Key('contact-notes'),
                controller: notes,
                decoration: const InputDecoration(
                  labelText: 'Notizen',
                  alignLabelWithHint: true,
                ),
                minLines: 3,
                maxLines: 8,
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _groupTitle(BuildContext context, String title) {
    return Padding(
      padding: const EdgeInsets.only(top: 18, bottom: 6),
      child: Text(
        title.toUpperCase(),
        style: TextStyle(
          fontSize: 10,
          fontWeight: FontWeight.w700,
          color: Theme.of(context).colorScheme.primary,
        ),
      ),
    );
  }

  Widget _labeledRow({
    required _LabeledField field,
    required List<ContactLabel> labels,
    required Key fieldKey,
    required String hint,
    required TextInputType keyboardType,
    required VoidCallback onRemove,
    String? Function(String?)? validator,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 150,
            child: DropdownButtonFormField<ContactLabel>(
              isExpanded: true,
              initialValue: labels.contains(field.label)
                  ? field.label
                  : labels.first,
              items: [
                for (final label in labels)
                  DropdownMenuItem(value: label, child: Text(label.label)),
              ],
              onChanged: (value) =>
                  setState(() => field.label = value ?? field.label),
            ),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: TextFormField(
              key: fieldKey,
              controller: field.controller,
              decoration: InputDecoration(hintText: hint),
              keyboardType: keyboardType,
              validator: validator,
            ),
          ),
          IconButton(
            tooltip: 'Entfernen',
            icon: const Icon(Icons.close, size: 16),
            onPressed: onRemove,
          ),
        ],
      ),
    );
  }

  Widget _addButton({
    required Key key,
    required String label,
    required VoidCallback onTap,
  }) {
    return Align(
      alignment: Alignment.centerLeft,
      child: TextButton.icon(
        key: key,
        onPressed: onTap,
        icon: const Icon(Icons.add, size: 16),
        label: Text(label),
      ),
    );
  }

  Widget _addressBlock(int index) {
    final address = addresses[index];
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).dividerColor),
        borderRadius: BorderRadius.circular(3),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              SizedBox(
                width: 150,
                child: DropdownButtonFormField<ContactLabel>(
                  isExpanded: true,
                  initialValue: address.label,
                  items: [
                    for (final label in ContactLabel.addressLabels)
                      DropdownMenuItem(value: label, child: Text(label.label)),
                  ],
                  onChanged: (value) =>
                      setState(() => address.label = value ?? address.label),
                ),
              ),
              const Spacer(),
              IconButton(
                tooltip: 'Adresse entfernen',
                icon: const Icon(Icons.close, size: 16),
                onPressed: () =>
                    setState(() => addresses.removeAt(index).dispose()),
              ),
            ],
          ),
          const SizedBox(height: 8),
          TextFormField(
            key: Key('contact-address-street-$index'),
            controller: address.street,
            decoration: const InputDecoration(
              labelText: 'Straße und Hausnummer',
            ),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              SizedBox(
                width: 120,
                child: TextFormField(
                  key: Key('contact-address-postal-code-$index'),
                  controller: address.postalCode,
                  decoration: const InputDecoration(labelText: 'PLZ'),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: TextFormField(
                  key: Key('contact-address-city-$index'),
                  controller: address.city,
                  decoration: const InputDecoration(labelText: 'Ort'),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: TextFormField(
                  key: Key('contact-address-country-$index'),
                  controller: address.country,
                  decoration: const InputDecoration(labelText: 'Land'),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
