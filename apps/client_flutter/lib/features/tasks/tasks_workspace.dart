import 'package:flutter/material.dart';

import '../../app_theme.dart';
import '../workspace/double_click.dart';
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

/// Lists offered in the task navigation pane.
enum TaskListFilter {
  all('Alle Aufgaben', Icons.checklist_outlined),
  today('Heute', Icons.today_outlined),
  overdue('Überfällig', Icons.warning_amber_outlined),
  upcoming('Nächste 7 Tage', Icons.date_range_outlined),
  completed('Erledigt', Icons.task_alt_outlined);

  const TaskListFilter(this.label, this.icon);

  final String label;
  final IconData icon;

  bool accepts(LocalTaskItem task, DateTime today) {
    final due = task.dueAt == null ? null : dateOnly(task.dueAt!);
    switch (this) {
      case TaskListFilter.all:
        return true;
      case TaskListFilter.completed:
        return task.done;
      case TaskListFilter.today:
        return !task.done && due != null && !due.isAfter(today);
      case TaskListFilter.overdue:
        return !task.done && due != null && due.isBefore(today);
      case TaskListFilter.upcoming:
        return !task.done &&
            due != null &&
            !due.isBefore(today) &&
            !due.isAfter(today.add(const Duration(days: 7)));
    }
  }
}

/// Section of the task list, in display order.
enum _TaskGroup {
  overdue('Überfällig'),
  today('Heute'),
  tomorrow('Morgen'),
  thisWeek('Diese Woche'),
  later('Später'),
  undated('Ohne Fälligkeitsdatum'),
  completed('Erledigt');

  const _TaskGroup(this.label);

  final String label;

  static _TaskGroup of(LocalTaskItem task, DateTime today) {
    if (task.done) return _TaskGroup.completed;
    final due = task.dueAt;
    if (due == null) return _TaskGroup.undated;
    final difference = dateOnly(due).difference(today).inDays;
    if (difference < 0) return _TaskGroup.overdue;
    if (difference == 0) return _TaskGroup.today;
    if (difference == 1) return _TaskGroup.tomorrow;
    if (dateOnly(
      due,
    ).isBefore(startOfWeek(today).add(const Duration(days: 7)))) {
      return _TaskGroup.thisWeek;
    }
    return _TaskGroup.later;
  }
}

/// Navigation entries for the task module's left pane.
class TaskFilterList extends StatelessWidget {
  const TaskFilterList({
    super.key,
    required this.tasks,
    required this.selected,
    required this.onSelected,
    this.today,
  });

  final List<LocalTaskItem> tasks;
  final TaskListFilter selected;
  final ValueChanged<TaskListFilter> onSelected;
  final DateTime? today;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final now = dateOnly(today ?? DateTime.now());
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final filter in TaskListFilter.values)
          InkWell(
            key: Key('task-filter-${filter.name}'),
            onTap: () => onSelected(filter),
            child: Container(
              height: 29,
              padding: const EdgeInsets.symmetric(horizontal: 17),
              color: filter == selected ? palette.selected : null,
              child: Row(
                children: [
                  Icon(
                    filter.icon,
                    size: 16,
                    color: filter == selected
                        ? maicentaPrimaryBlue
                        : palette.mutedText,
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      filter.label,
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: filter == selected
                            ? FontWeight.w600
                            : FontWeight.normal,
                      ),
                    ),
                  ),
                  Text(
                    '${tasks.where((task) => filter.accepts(task, now)).length}',
                    style: TextStyle(fontSize: 11, color: palette.mutedText),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

/// Task list with quick entry on the left, details of the selected task on
/// the right.
class TasksWorkspace extends StatefulWidget {
  const TasksWorkspace({
    super.key,
    required this.tasks,
    required this.filter,
    required this.selectedId,
    required this.onSelected,
    required this.onToggle,
    required this.onOpen,
    required this.onDelete,
    required this.onQuickAdd,
    this.today,
  });

  final List<LocalTaskItem> tasks;
  final TaskListFilter filter;
  final String? selectedId;
  final ValueChanged<LocalTaskItem> onSelected;
  final ValueChanged<LocalTaskItem> onToggle;
  final ValueChanged<LocalTaskItem> onOpen;
  final ValueChanged<LocalTaskItem> onDelete;
  final ValueChanged<String> onQuickAdd;
  final DateTime? today;

  @override
  State<TasksWorkspace> createState() => _TasksWorkspaceState();
}

class _TasksWorkspaceState extends State<TasksWorkspace> {
  final quickAdd = TextEditingController();

  @override
  void dispose() {
    quickAdd.dispose();
    super.dispose();
  }

  void submitQuickAdd() {
    final title = quickAdd.text.trim();
    if (title.isEmpty) return;
    quickAdd.clear();
    widget.onQuickAdd(title);
  }

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final today = dateOnly(widget.today ?? DateTime.now());
    final visible = widget.tasks
        .where((task) => widget.filter.accepts(task, today))
        .toList(growable: false);
    final groups = <_TaskGroup, List<LocalTaskItem>>{};
    for (final task in visible) {
      groups.putIfAbsent(_TaskGroup.of(task, today), () => []).add(task);
    }
    final selected = widget.selectedId == null
        ? null
        : widget.tasks.cast<LocalTaskItem?>().firstWhere(
            (task) => task!.id == widget.selectedId,
            orElse: () => null,
          );
    final openCount = widget.tasks.where((task) => !task.done).length;
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
              Text(
                widget.filter.label,
                style: const TextStyle(
                  fontSize: 17,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const Spacer(),
              Text(
                openCount == 1
                    ? '1 offene Aufgabe'
                    : '$openCount offene Aufgaben',
                style: TextStyle(color: palette.mutedText, fontSize: 12),
              ),
            ],
          ),
        ),
        Expanded(
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(
                flex: 5,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                      child: TextField(
                        key: const Key('task-quick-add'),
                        controller: quickAdd,
                        decoration: InputDecoration(
                          hintText: 'Neue Aufgabe eingeben und Eingabe drücken',
                          prefixIcon: const Icon(Icons.add, size: 18),
                          suffixIcon: IconButton(
                            key: const Key('task-quick-add-submit'),
                            tooltip: 'Aufgabe anlegen',
                            icon: const Icon(Icons.keyboard_return, size: 18),
                            onPressed: submitQuickAdd,
                          ),
                        ),
                        onSubmitted: (_) => submitQuickAdd(),
                      ),
                    ),
                    Expanded(
                      child: visible.isEmpty
                          ? Center(
                              child: Text(
                                'Keine Aufgaben in dieser Liste.',
                                style: TextStyle(color: palette.mutedText),
                              ),
                            )
                          : ListView(
                              padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
                              children: [
                                for (final group in _TaskGroup.values)
                                  if (groups[group] case final tasks?) ...[
                                    Padding(
                                      padding: const EdgeInsets.fromLTRB(
                                        2,
                                        12,
                                        2,
                                        4,
                                      ),
                                      child: Text(
                                        '${group.label} (${tasks.length})',
                                        style: TextStyle(
                                          fontSize: 11,
                                          fontWeight: FontWeight.w700,
                                          color: group == _TaskGroup.overdue
                                              ? Theme.of(
                                                  context,
                                                ).colorScheme.error
                                              : palette.mutedText,
                                        ),
                                      ),
                                    ),
                                    for (final task in tasks)
                                      TaskRow(
                                        task: task,
                                        selected: task.id == widget.selectedId,
                                        onTap: () => widget.onSelected(task),
                                        onDoubleTap: () => widget.onOpen(task),
                                        onToggle: () => widget.onToggle(task),
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
                flex: 4,
                child: selected == null
                    ? Center(
                        child: Text(
                          'Keine Aufgabe ausgewählt.',
                          style: TextStyle(color: palette.mutedText),
                        ),
                      )
                    : TaskDetails(
                        task: selected,
                        onEdit: () => widget.onOpen(selected),
                        onToggle: () => widget.onToggle(selected),
                        onDelete: () => widget.onDelete(selected),
                      ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class TaskRow extends StatelessWidget {
  const TaskRow({
    super.key,
    required this.task,
    required this.selected,
    required this.onTap,
    required this.onDoubleTap,
    required this.onToggle,
  });

  final LocalTaskItem task;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback onDoubleTap;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final error = Theme.of(context).colorScheme.error;
    return DoubleClickInkWell(
      key: Key('task-row-${task.id}'),
      onTap: onTap,
      onDoubleTap: onDoubleTap,
      child: Container(
        margin: const EdgeInsets.only(bottom: 4),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: selected ? palette.selected : palette.pane,
          border: Border.all(
            color: selected ? maicentaPrimaryBlue : palette.border,
          ),
        ),
        child: Row(
          children: [
            Checkbox(
              key: Key('task-check-${task.id}'),
              value: task.done,
              visualDensity: VisualDensity.compact,
              onChanged: (_) => onToggle(),
            ),
            const SizedBox(width: 4),
            if (task.priority == TaskPriority.high)
              Padding(
                padding: const EdgeInsets.only(right: 6),
                child: Icon(Icons.priority_high, size: 16, color: error),
              )
            else if (task.priority == TaskPriority.low)
              Padding(
                padding: const EdgeInsets.only(right: 6),
                child: Icon(
                  Icons.arrow_downward,
                  size: 14,
                  color: palette.mutedText,
                ),
              ),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    task.title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      decoration: task.done ? TextDecoration.lineThrough : null,
                      color: task.done ? palette.mutedText : null,
                    ),
                  ),
                  if (task.status != TaskStatus.notStarted &&
                      task.status != TaskStatus.completed)
                    Text(
                      task.status.label,
                      style: TextStyle(fontSize: 10, color: palette.mutedText),
                    ),
                ],
              ),
            ),
            if (task.reminderAt != null && !task.done)
              Padding(
                padding: const EdgeInsets.only(right: 8),
                child: Icon(
                  Icons.notifications_outlined,
                  size: 14,
                  color: palette.mutedText,
                ),
              ),
            Text(
              task.due,
              style: TextStyle(
                fontSize: 11,
                color: task.overdue ? error : palette.mutedText,
                fontWeight: task.overdue ? FontWeight.w600 : FontWeight.normal,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class TaskDetails extends StatelessWidget {
  const TaskDetails({
    super.key,
    required this.task,
    required this.onEdit,
    required this.onToggle,
    required this.onDelete,
  });

  final LocalTaskItem task;
  final VoidCallback onEdit;
  final VoidCallback onToggle;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            task.title,
            key: const Key('task-detail-title'),
            style: TextStyle(
              fontSize: 18,
              fontWeight: FontWeight.w600,
              decoration: task.done ? TextDecoration.lineThrough : null,
            ),
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              FilledButton.tonalIcon(
                key: const Key('task-toggle'),
                onPressed: onToggle,
                icon: Icon(task.done ? Icons.replay : Icons.check, size: 18),
                label: Text(
                  task.done ? 'Erneut öffnen' : 'Als erledigt markieren',
                ),
              ),
              OutlinedButton.icon(
                key: const Key('task-edit'),
                onPressed: onEdit,
                icon: const Icon(Icons.edit_outlined, size: 18),
                label: const Text('Bearbeiten'),
              ),
              OutlinedButton.icon(
                key: const Key('task-delete'),
                onPressed: onDelete,
                icon: const Icon(Icons.delete_outline, size: 18),
                label: const Text('Löschen'),
              ),
            ],
          ),
          const SizedBox(height: 20),
          _DetailLine(
            icon: Icons.flag_outlined,
            label: 'Status',
            value: task.status.label,
          ),
          _DetailLine(
            icon: Icons.low_priority,
            label: 'Priorität',
            value: task.priority.label,
          ),
          if (task.startsAt case final start?)
            _DetailLine(
              icon: Icons.play_arrow_outlined,
              label: 'Beginnt am',
              value: formatLongDate(start),
            ),
          _DetailLine(
            icon: Icons.event_outlined,
            label: 'Fällig am',
            value: task.dueAt == null
                ? 'Kein Datum'
                : '${formatLongDate(task.dueAt!)}'
                      '${task.overdue ? ' · überfällig' : ''}',
            highlight: task.overdue,
          ),
          _DetailLine(
            icon: Icons.notifications_outlined,
            label: 'Erinnerung',
            value: task.reminderAt == null
                ? 'Keine'
                : '${formatDateTime(task.reminderAt!)}'
                      '${task.reminderDismissed ? ' · geschlossen' : ''}',
          ),
          if (task.completedAt case final completed?)
            _DetailLine(
              icon: Icons.task_alt_outlined,
              label: 'Erledigt am',
              value: formatDateTime(completed),
            ),
          const SizedBox(height: 16),
          Text(
            'NOTIZEN',
            style: TextStyle(
              fontSize: 10,
              fontWeight: FontWeight.w700,
              color: palette.mutedText,
            ),
          ),
          const SizedBox(height: 6),
          SelectableText(
            task.description?.isNotEmpty == true
                ? task.description!
                : 'Keine Notizen.',
            style: TextStyle(
              color: task.description?.isNotEmpty == true
                  ? null
                  : palette.mutedText,
            ),
          ),
        ],
      ),
    );
  }
}

class _DetailLine extends StatelessWidget {
  const _DetailLine({
    required this.icon,
    required this.label,
    required this.value,
    this.highlight = false,
  });

  final IconData icon;
  final String label;
  final String value;
  final bool highlight;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
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
          Expanded(
            child: SelectableText(
              value,
              style: TextStyle(
                fontSize: 12,
                color: highlight ? Theme.of(context).colorScheme.error : null,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
