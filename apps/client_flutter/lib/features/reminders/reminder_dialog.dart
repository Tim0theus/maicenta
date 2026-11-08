import 'package:flutter/material.dart';

import '../../app_theme.dart';

enum ReminderKind { event, task }

/// One due reminder shown in the reminder window.
class ReminderEntry {
  const ReminderEntry({
    required this.id,
    required this.itemId,
    required this.kind,
    required this.title,
    required this.detail,
    required this.dueAt,
    this.occurrenceStart,
  });

  /// Unique key of the reminder: the item identifier, with the occurrence
  /// date appended for recurring events.
  final String id;

  /// Identifier of the event or task.
  final String itemId;
  final ReminderKind kind;

  /// Start of the occurrence the reminder belongs to (events only).
  final DateTime? occurrenceStart;
  final String title;

  /// Second line, for example the start time or the due date.
  final String detail;

  /// Start of the event or due time of the task, for the "overdue by" label.
  final DateTime dueAt;
}

/// Snooze choices offered in the reminder window.
const reminderSnoozeChoices = <(String, Duration)>[
  ('5 Minuten', Duration(minutes: 5)),
  ('10 Minuten', Duration(minutes: 10)),
  ('15 Minuten', Duration(minutes: 15)),
  ('30 Minuten', Duration(minutes: 30)),
  ('1 Stunde', Duration(hours: 1)),
  ('4 Stunden', Duration(hours: 4)),
  ('1 Tag', Duration(days: 1)),
];

/// Shows the reminder window for the given entries. Callbacks run as the
/// user acts; the window closes once every entry is handled or hidden.
Future<void> showReminderDialog(
  BuildContext context, {
  required List<ReminderEntry> entries,
  required Future<void> Function(ReminderEntry entry) onDismiss,
  required void Function(ReminderEntry entry, Duration delay) onSnooze,
  required void Function(ReminderEntry entry) onOpen,
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) => _ReminderDialog(
      entries: entries,
      onDismiss: onDismiss,
      onSnooze: onSnooze,
      onOpen: onOpen,
    ),
  );
}

String overdueLabel(DateTime dueAt, DateTime now) {
  final difference = now.difference(dueAt);
  if (difference.inMinutes < 1) return 'jetzt';
  if (difference.inMinutes < 60) {
    return 'seit ${difference.inMinutes} Minuten';
  }
  if (difference.inHours < 24) return 'seit ${difference.inHours} Stunden';
  return difference.inDays == 1
      ? 'seit 1 Tag'
      : 'seit ${difference.inDays} Tagen';
}

class _ReminderDialog extends StatefulWidget {
  const _ReminderDialog({
    required this.entries,
    required this.onDismiss,
    required this.onSnooze,
    required this.onOpen,
  });

  final List<ReminderEntry> entries;
  final Future<void> Function(ReminderEntry entry) onDismiss;
  final void Function(ReminderEntry entry, Duration delay) onSnooze;
  final void Function(ReminderEntry entry) onOpen;

  @override
  State<_ReminderDialog> createState() => _ReminderDialogState();
}

class _ReminderDialogState extends State<_ReminderDialog> {
  late final List<ReminderEntry> pending = widget.entries.toList();

  void remove(ReminderEntry entry) {
    setState(() => pending.removeWhere((item) => item.id == entry.id));
    if (pending.isEmpty && mounted) Navigator.pop(context);
  }

  Future<void> dismiss(ReminderEntry entry) async {
    await widget.onDismiss(entry);
    if (mounted) remove(entry);
  }

  Future<void> dismissAll() async {
    for (final entry in pending.toList()) {
      await widget.onDismiss(entry);
    }
    if (mounted) Navigator.pop(context);
  }

  void snooze(ReminderEntry entry, Duration delay) {
    widget.onSnooze(entry, delay);
    remove(entry);
  }

  void hideAll() {
    for (final entry in pending.toList()) {
      widget.onSnooze(entry, const Duration(minutes: 5));
    }
    Navigator.pop(context);
  }

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final now = DateTime.now();
    return AlertDialog(
      key: const Key('reminder-dialog'),
      title: Row(
        children: [
          const Icon(Icons.notifications_active_outlined, size: 22),
          const SizedBox(width: 10),
          Text(
            pending.length == 1
                ? '1 Erinnerung'
                : '${pending.length} Erinnerungen',
          ),
        ],
      ),
      content: SizedBox(
        width: 520,
        child: ListView.separated(
          shrinkWrap: true,
          itemCount: pending.length,
          separatorBuilder: (_, _) => const Divider(height: 1),
          itemBuilder: (context, index) {
            final entry = pending[index];
            return Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Icon(
                    entry.kind == ReminderKind.event
                        ? Icons.event_outlined
                        : Icons.check_box_outlined,
                    size: 20,
                    color: maicentaPrimaryBlue,
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: InkWell(
                      key: Key('reminder-open-${entry.id}'),
                      onTap: () {
                        widget.onOpen(entry);
                      },
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            entry.title,
                            style: const TextStyle(fontWeight: FontWeight.w600),
                          ),
                          Text(
                            entry.detail,
                            style: TextStyle(
                              fontSize: 12,
                              color: palette.mutedText,
                            ),
                          ),
                          Text(
                            'Fällig ${overdueLabel(entry.dueAt, now)}',
                            style: TextStyle(
                              fontSize: 11,
                              color: palette.mutedText,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  PopupMenuButton<Duration>(
                    key: Key('reminder-snooze-${entry.id}'),
                    tooltip: 'Erneut erinnern',
                    onSelected: (delay) => snooze(entry, delay),
                    itemBuilder: (context) => [
                      for (final (label, delay) in reminderSnoozeChoices)
                        PopupMenuItem(
                          value: delay,
                          child: Text('In $label erneut erinnern'),
                        ),
                    ],
                    child: const Padding(
                      padding: EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(Icons.snooze_outlined, size: 16),
                          SizedBox(width: 4),
                          Text('Später', style: TextStyle(fontSize: 12)),
                          Icon(Icons.arrow_drop_down, size: 16),
                        ],
                      ),
                    ),
                  ),
                  TextButton(
                    key: Key('reminder-dismiss-${entry.id}'),
                    onPressed: () => dismiss(entry),
                    child: const Text('Schließen'),
                  ),
                ],
              ),
            );
          },
        ),
      ),
      actions: [
        TextButton(
          key: const Key('reminder-hide-all'),
          onPressed: hideAll,
          child: const Text('Ausblenden'),
        ),
        FilledButton(
          key: const Key('reminder-dismiss-all'),
          onPressed: dismissAll,
          child: const Text('Alle schließen'),
        ),
      ],
    );
  }
}
