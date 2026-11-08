import 'package:flutter/material.dart';

import '../../app_theme.dart';
import '../workspace/double_click.dart';
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

/// Up to two initials of a display name.
String contactInitials(String name) {
  final parts = name
      .trim()
      .split(RegExp(r'\s+'))
      .where((part) => part.isNotEmpty);
  return parts.take(2).map((part) => part[0].toUpperCase()).join();
}

/// Address book: searchable list on the left, contact card on the right.
class ContactsWorkspace extends StatelessWidget {
  const ContactsWorkspace({
    super.key,
    required this.contacts,
    required this.query,
    required this.onQueryChanged,
    required this.selectedId,
    required this.onSelected,
    required this.onOpen,
    required this.onDelete,
    required this.onCompose,
  });

  final List<LocalContactItem> contacts;
  final String query;
  final ValueChanged<String> onQueryChanged;
  final String? selectedId;
  final ValueChanged<LocalContactItem> onSelected;
  final ValueChanged<LocalContactItem> onOpen;
  final ValueChanged<LocalContactItem> onDelete;

  /// Starts a new mail to the given address of the contact.
  final void Function(LocalContactItem contact, String address) onCompose;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final visible = contacts.where((contact) => contact.matches(query)).toList()
      ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    final selected = selectedId == null
        ? null
        : contacts.cast<LocalContactItem?>().firstWhere(
            (contact) => contact!.id == selectedId,
            orElse: () => null,
          );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 54,
          padding: const EdgeInsets.symmetric(horizontal: 20),
          decoration: BoxDecoration(
            color: palette.pane,
            border: Border(bottom: BorderSide(color: palette.border)),
          ),
          child: Row(
            children: [
              const Text(
                'Kontakte',
                style: TextStyle(fontSize: 17, fontWeight: FontWeight.w600),
              ),
              const Spacer(),
              Text(
                contacts.length == 1
                    ? '1 Kontakt'
                    : '${contacts.length} Kontakte',
                style: TextStyle(color: palette.mutedText, fontSize: 12),
              ),
            ],
          ),
        ),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SizedBox(
                width: 320,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(12, 12, 12, 6),
                      child: TextField(
                        key: const Key('contact-search'),
                        decoration: const InputDecoration(
                          hintText: 'Kontakte durchsuchen',
                          prefixIcon: Icon(Icons.search, size: 18),
                        ),
                        onChanged: onQueryChanged,
                      ),
                    ),
                    Expanded(
                      child: visible.isEmpty
                          ? Center(
                              child: Text(
                                contacts.isEmpty
                                    ? 'Noch keine Kontakte.'
                                    : 'Keine Treffer.',
                                style: TextStyle(color: palette.mutedText),
                              ),
                            )
                          : ListView(
                              padding: const EdgeInsets.only(bottom: 12),
                              children: [
                                for (
                                  var index = 0;
                                  index < visible.length;
                                  index++
                                ) ...[
                                  if (index == 0 ||
                                      _letter(visible[index - 1].name) !=
                                          _letter(visible[index].name))
                                    Padding(
                                      padding: const EdgeInsets.fromLTRB(
                                        16,
                                        10,
                                        16,
                                        2,
                                      ),
                                      child: Text(
                                        _letter(visible[index].name),
                                        style: TextStyle(
                                          fontSize: 11,
                                          fontWeight: FontWeight.w700,
                                          color: maicentaPrimaryBlue,
                                        ),
                                      ),
                                    ),
                                  ContactRow(
                                    contact: visible[index],
                                    selected: visible[index].id == selectedId,
                                    onTap: () => onSelected(visible[index]),
                                    onDoubleTap: () => onOpen(visible[index]),
                                  ),
                                ],
                              ],
                            ),
                    ),
                  ],
                ),
              ),
              const VerticalDivider(width: 1),
              Expanded(
                child: selected == null
                    ? Center(
                        child: Text(
                          'Kein Kontakt ausgewählt.',
                          style: TextStyle(color: palette.mutedText),
                        ),
                      )
                    : ContactCard(
                        contact: selected,
                        onEdit: () => onOpen(selected),
                        onDelete: () => onDelete(selected),
                        onCompose: (address) => onCompose(selected, address),
                      ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  static String _letter(String name) {
    final trimmed = name.trim();
    if (trimmed.isEmpty) return '#';
    final first = trimmed[0].toUpperCase();
    return RegExp(r'[A-ZÄÖÜ]').hasMatch(first) ? first : '#';
  }
}

class ContactRow extends StatelessWidget {
  const ContactRow({
    super.key,
    required this.contact,
    required this.selected,
    required this.onTap,
    required this.onDoubleTap,
  });

  final LocalContactItem contact;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback onDoubleTap;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return DoubleClickInkWell(
      key: Key('contact-row-${contact.id}'),
      onTap: onTap,
      onDoubleTap: onDoubleTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
        color: selected ? palette.selected : null,
        child: Row(
          children: [
            CircleAvatar(
              radius: 16,
              backgroundColor: selected
                  ? maicentaPrimaryBlue
                  : palette.selectedStrong,
              child: Text(
                contactInitials(contact.name),
                style: TextStyle(
                  fontSize: 12,
                  color: selected ? Colors.white : maicentaPrimaryBlue,
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    contact.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                  if (contact.subtitle.isNotEmpty)
                    Text(
                      contact.subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 11, color: palette.mutedText),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Full contact card with every stored detail.
class ContactCard extends StatelessWidget {
  const ContactCard({
    super.key,
    required this.contact,
    required this.onEdit,
    required this.onDelete,
    required this.onCompose,
  });

  final LocalContactItem contact;
  final VoidCallback onEdit;
  final VoidCallback onDelete;
  final ValueChanged<String> onCompose;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final headline = [
      if (contact.jobTitle case final title? when title.isNotEmpty) title,
      if (contact.company case final company? when company.isNotEmpty) company,
    ].join(' · ');
    return SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              CircleAvatar(
                radius: 32,
                backgroundColor: palette.selectedStrong,
                child: Text(
                  contactInitials(contact.name),
                  style: const TextStyle(
                    fontSize: 22,
                    color: maicentaPrimaryBlue,
                  ),
                ),
              ),
              const SizedBox(width: 18),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SelectableText(
                      contact.name,
                      key: const Key('contact-detail-name'),
                      style: const TextStyle(
                        fontSize: 20,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    if (headline.isNotEmpty)
                      SelectableText(
                        headline,
                        style: TextStyle(color: palette.mutedText),
                      ),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              FilledButton.tonalIcon(
                key: const Key('contact-compose'),
                onPressed: contact.emails.isEmpty
                    ? null
                    : () => onCompose(contact.emails.first.address),
                icon: const Icon(Icons.mail_outline, size: 18),
                label: const Text('E-Mail senden'),
              ),
              OutlinedButton.icon(
                key: const Key('contact-edit'),
                onPressed: onEdit,
                icon: const Icon(Icons.edit_outlined, size: 18),
                label: const Text('Bearbeiten'),
              ),
              OutlinedButton.icon(
                key: const Key('contact-delete'),
                onPressed: onDelete,
                icon: const Icon(Icons.delete_outline, size: 18),
                label: const Text('Löschen'),
              ),
            ],
          ),
          const SizedBox(height: 20),
          if (contact.emails.isNotEmpty)
            _Section(
              title: 'E-MAIL',
              children: [
                for (final email in contact.emails)
                  _DetailRow(
                    icon: Icons.alternate_email,
                    label: email.label.label,
                    value: email.address,
                    action: IconButton(
                      tooltip: 'E-Mail an ${email.address}',
                      icon: const Icon(Icons.send_outlined, size: 16),
                      onPressed: () => onCompose(email.address),
                    ),
                  ),
              ],
            ),
          if (contact.phones.isNotEmpty)
            _Section(
              title: 'TELEFON',
              children: [
                for (final phone in contact.phones)
                  _DetailRow(
                    icon: phone.label == ContactLabel.mobile
                        ? Icons.smartphone_outlined
                        : phone.label == ContactLabel.fax
                        ? Icons.fax_outlined
                        : Icons.phone_outlined,
                    label: phone.label.label,
                    value: phone.number,
                  ),
              ],
            ),
          if (contact.addresses.isNotEmpty)
            _Section(
              title: 'ADRESSE',
              children: [
                for (final address in contact.addresses)
                  _DetailRow(
                    icon: address.label == ContactLabel.home
                        ? Icons.home_outlined
                        : Icons.business_outlined,
                    label: address.label.label,
                    value: address.lines.join('\n'),
                  ),
              ],
            ),
          if (contact.birthday != null || contact.website != null)
            _Section(
              title: 'WEITERES',
              children: [
                if (contact.birthday case final birthday?)
                  _DetailRow(
                    icon: Icons.cake_outlined,
                    label: 'Geburtstag',
                    value: formatDate(birthday),
                  ),
                if (contact.website case final website?)
                  _DetailRow(
                    icon: Icons.link_outlined,
                    label: 'Webseite',
                    value: website,
                  ),
              ],
            ),
          _Section(
            title: 'NOTIZEN',
            children: [
              SelectableText(
                contact.notes?.isNotEmpty == true
                    ? contact.notes!
                    : 'Keine Notizen.',
                style: TextStyle(
                  color: contact.notes?.isNotEmpty == true
                      ? null
                      : palette.mutedText,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.children});

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: TextStyle(
              fontSize: 10,
              fontWeight: FontWeight.w700,
              color: palette.mutedText,
            ),
          ),
          const SizedBox(height: 6),
          ...children,
        ],
      ),
    );
  }
}

class _DetailRow extends StatelessWidget {
  const _DetailRow({
    required this.icon,
    required this.label,
    required this.value,
    this.action,
  });

  final IconData icon;
  final String label;
  final String value;
  final Widget? action;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 16, color: palette.mutedText),
          const SizedBox(width: 8),
          SizedBox(
            width: 90,
            child: Text(
              label,
              style: TextStyle(fontSize: 12, color: palette.mutedText),
            ),
          ),
          Expanded(child: SelectableText(value)),
          ?action,
        ],
      ),
    );
  }
}
