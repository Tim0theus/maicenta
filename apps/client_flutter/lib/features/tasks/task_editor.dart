import 'package:flutter/material.dart';

import '../calendar/event_editor.dart' show EditorAction;
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

class TaskEditorResult {
  const TaskEditorResult(this.action, this.task);

  final EditorAction action;
  final LocalTaskItem task;
}

/// Opens the task window for a new or existing task.
Future<TaskEditorResult?> showTaskEditor(
  BuildContext context, {
  LocalTaskItem? task,
  String initialTitle = '',
  List<WorkspaceCollection> lists = const [],
}) {
  return showDialog<TaskEditorResult>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) =>
        _TaskEditor(task: task, initialTitle: initialTitle, lists: lists),
  );
}

class _TaskEditor extends StatefulWidget {
  const _TaskEditor({
    required this.task,
    required this.initialTitle,
    required this.lists,
  });

  final LocalTaskItem? task;
  final String initialTitle;

  /// Synchronized task lists the task may be stored in besides the local one.
  final List<WorkspaceCollection> lists;

  @override
  State<_TaskEditor> createState() => _TaskEditorState();
}

class _TaskEditorState extends State<_TaskEditor> {
  final formKey = GlobalKey<FormState>();
  late final TextEditingController title;
  late final TextEditingController description;
  late final TextEditingController startDate;
  late final TextEditingController dueDate;
  late final TextEditingController reminderDate;
  late final TextEditingController reminderTime;
  late TaskStatus status;
  late TaskPriority priority;
  late bool reminderEnabled;
  String? rangeError;
  String? collectionId;

  @override
  void initState() {
    super.initState();
    final task = widget.task;
    collectionId = task?.collectionId;
    if (collectionId != null &&
        !widget.lists.any((list) => list.id == collectionId)) {
      collectionId = null;
    }
    title = TextEditingController(text: task?.title ?? widget.initialTitle);
    description = TextEditingController(text: task?.description ?? '');
    startDate = TextEditingController(
      text: task?.startsAt == null ? '' : formatDate(task!.startsAt!),
    );
    dueDate = TextEditingController(
      text: task?.dueAt == null ? '' : formatDate(task!.dueAt!),
    );
    final reminder = task?.reminderAt;
    reminderEnabled = reminder != null;
    reminderDate = TextEditingController(
      text: reminder == null ? '' : formatDate(reminder),
    );
    reminderTime = TextEditingController(
      text: reminder == null ? '' : formatClock(reminder),
    );
    status = task?.status ?? TaskStatus.notStarted;
    priority = task?.priority ?? TaskPriority.normal;
  }

  @override
  void dispose() {
    for (final controller in [
      title,
      description,
      startDate,
      dueDate,
      reminderDate,
      reminderTime,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  void toggleReminder(bool enabled) {
    setState(() {
      reminderEnabled = enabled;
      if (enabled && reminderDate.text.trim().isEmpty) {
        final due = parseGermanDate(dueDate.text);
        final seed =
            due ?? dateOnly(DateTime.now()).add(const Duration(days: 1));
        reminderDate.text = formatDate(seed);
        reminderTime.text = '09:00';
      }
    });
  }

  void save() {
    if (!(formKey.currentState?.validate() ?? false)) return;
    final start = parseGermanDate(startDate.text);
    final due = parseGermanDate(dueDate.text);
    if (start != null && due != null && due.isBefore(start)) {
      setState(
        () => rangeError =
            'Das Fälligkeitsdatum darf nicht vor dem Beginn liegen.',
      );
      return;
    }
    DateTime? reminderAt;
    if (reminderEnabled) {
      final day = parseGermanDate(reminderDate.text);
      final clock = parseClock(reminderTime.text);
      if (day == null || clock == null) return;
      reminderAt = combine(day, clock);
    }
    final original = widget.task;
    final now = DateTime.now();
    final completed = status == TaskStatus.completed;
    final task = LocalTaskItem(
      id: original?.id ?? 'local.task.${now.microsecondsSinceEpoch}',
      title: title.text.trim(),
      description: description.text.trim().isEmpty
          ? null
          : description.text.trim(),
      startsAt: start,
      dueAt: due,
      status: status,
      priority: priority,
      reminderAt: reminderAt,
      reminderDismissed: original != null && original.reminderAt == reminderAt
          ? original.reminderDismissed
          : false,
      completedAt: completed ? (original?.completedAt ?? now) : null,
      collectionId: collectionId,
    );
    Navigator.pop(context, TaskEditorResult(EditorAction.save, task));
  }

  Future<void> delete() async {
    final task = widget.task;
    if (task == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Aufgabe löschen?'),
        content: Text('„${task.title}“ wird endgültig entfernt.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            key: const Key('task-delete-confirm'),
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Löschen'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    Navigator.pop(context, TaskEditorResult(EditorAction.delete, task));
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.task != null;
    return EditorDialog(
      title: editing ? 'Aufgabe bearbeiten' : 'Neue Aufgabe',
      icon: Icons.check_box_outlined,
      leading: [
        if (editing)
          TextButton.icon(
            key: const Key('task-editor-delete'),
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
          key: const Key('task-save'),
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
                key: const Key('task-title'),
                controller: title,
                autofocus: !editing,
                decoration: const InputDecoration(labelText: 'Betreff'),
                textInputAction: TextInputAction.next,
                validator: (value) => (value?.trim().isEmpty ?? true)
                    ? 'Bitte einen Betreff eingeben.'
                    : null,
              ),
              if (widget.lists.isNotEmpty) ...[
                const SizedBox(height: 12),
                DropdownButtonFormField<String?>(
                  key: const Key('task-list'),
                  isExpanded: true,
                  initialValue: collectionId,
                  decoration: const InputDecoration(
                    labelText: 'Aufgabenliste',
                    prefixIcon: Icon(Icons.list_alt_outlined, size: 18),
                  ),
                  items: [
                    const DropdownMenuItem<String?>(
                      value: null,
                      child: Text('Meine Aufgaben (lokal)'),
                    ),
                    for (final list in widget.lists)
                      DropdownMenuItem<String?>(
                        value: list.id,
                        child: Text(list.name, overflow: TextOverflow.ellipsis),
                      ),
                  ],
                  onChanged: (value) => setState(() => collectionId = value),
                ),
              ],
              const SizedBox(height: 12),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: DateField(
                      fieldKey: const Key('task-start-date'),
                      controller: startDate,
                      label: 'Beginnt am',
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: DateField(
                      fieldKey: const Key('task-due-date'),
                      controller: dueDate,
                      label: 'Fällig am',
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                ],
              ),
              if (rangeError case final error?)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    error,
                    key: const Key('task-range-error'),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                      fontSize: 12,
                    ),
                  ),
                ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: DropdownButtonFormField<TaskStatus>(
                      isExpanded: true,
                      key: const Key('task-status'),
                      initialValue: status,
                      decoration: const InputDecoration(labelText: 'Status'),
                      items: [
                        for (final value in TaskStatus.values)
                          DropdownMenuItem(
                            value: value,
                            child: Text(value.label),
                          ),
                      ],
                      onChanged: (value) =>
                          setState(() => status = value ?? status),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: DropdownButtonFormField<TaskPriority>(
                      isExpanded: true,
                      key: const Key('task-priority'),
                      initialValue: priority,
                      decoration: const InputDecoration(labelText: 'Priorität'),
                      items: [
                        for (final value in TaskPriority.values)
                          DropdownMenuItem(
                            value: value,
                            child: Text(value.label),
                          ),
                      ],
                      onChanged: (value) =>
                          setState(() => priority = value ?? priority),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              CheckboxListTile(
                key: const Key('task-reminder-enabled'),
                value: reminderEnabled,
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                secondary: const Icon(Icons.notifications_outlined, size: 18),
                title: const Text('Erinnerung'),
                onChanged: (value) => toggleReminder(value ?? false),
              ),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    flex: 3,
                    child: DateField(
                      fieldKey: const Key('task-reminder-date'),
                      controller: reminderDate,
                      label: 'Erinnerung am',
                      enabled: reminderEnabled,
                      required: reminderEnabled,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    flex: 2,
                    child: TimeField(
                      fieldKey: const Key('task-reminder-time'),
                      controller: reminderTime,
                      label: 'Uhrzeit',
                      enabled: reminderEnabled,
                      required: reminderEnabled,
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              TextFormField(
                key: const Key('task-description'),
                controller: description,
                decoration: const InputDecoration(
                  labelText: 'Notizen',
                  alignLabelWithHint: true,
                ),
                minLines: 4,
                maxLines: 10,
              ),
            ],
          ),
        ),
      ],
    );
  }
}
