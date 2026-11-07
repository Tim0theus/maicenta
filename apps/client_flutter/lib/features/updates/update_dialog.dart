import 'dart:async';

import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import 'app_updates.dart';

enum _UpdateStage { offered, downloading, ready, restarting, failed }

/// Offers [update], downloads it on request and restarts into the new
/// version. Returns once the dialog is closed without restarting.
Future<void> showAppUpdateDialog(
  BuildContext context, {
  required AppUpdateService service,
  required AvailableAppUpdate update,
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (_) => AppUpdateDialog(service: service, update: update),
  );
}

class AppUpdateDialog extends StatefulWidget {
  const AppUpdateDialog({
    super.key,
    required this.service,
    required this.update,
  });

  final AppUpdateService service;
  final AvailableAppUpdate update;

  @override
  State<AppUpdateDialog> createState() => _AppUpdateDialogState();
}

class _AppUpdateDialogState extends State<AppUpdateDialog> {
  _UpdateStage stage = _UpdateStage.offered;
  int progress = 0;
  String? failure;
  StreamSubscription<int>? download;

  bool get busy =>
      stage == _UpdateStage.downloading || stage == _UpdateStage.restarting;

  @override
  void dispose() {
    download?.cancel();
    super.dispose();
  }

  void startDownload() {
    setState(() {
      stage = _UpdateStage.downloading;
      progress = 0;
      failure = null;
    });
    download = widget.service.updater.download().listen(
      (percent) {
        if (mounted) setState(() => progress = percent.clamp(0, 100));
      },
      onError: (Object error) {
        if (mounted) {
          setState(() {
            stage = _UpdateStage.failed;
            failure = error.toString();
          });
        }
      },
      onDone: () {
        if (mounted && stage == _UpdateStage.downloading) {
          setState(() => stage = _UpdateStage.ready);
        }
      },
      cancelOnError: true,
    );
  }

  Future<void> restart() async {
    setState(() => stage = _UpdateStage.restarting);
    try {
      await widget.service.updater.applyAndRestart();
    } on Object catch (error) {
      if (!mounted) return;
      setState(() {
        stage = _UpdateStage.failed;
        failure = error.toString();
      });
    }
  }

  /// Manual installation: open the download (or the release page) in the
  /// browser and close the dialog.
  Future<void> openDownload() async {
    final link = widget.update.downloadUrl ?? widget.update.releasePageUrl;
    final uri = link == null ? null : Uri.tryParse(link);
    final opened =
        uri != null &&
        await launchUrl(uri, mode: LaunchMode.externalApplication);
    if (!mounted) return;
    if (opened) {
      Navigator.pop(context);
    } else {
      setState(() {
        stage = _UpdateStage.failed;
        failure = 'Der Download-Link konnte nicht geöffnet werden: $link';
      });
    }
  }

  Future<void> skip() async {
    await widget.service.skipVersion(widget.update.version);
    if (mounted) Navigator.pop(context);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final update = widget.update;
    final size = formatDownloadSize(update.downloadSizeBytes);
    return PopScope(
      canPop: !busy,
      child: AlertDialog(
        key: const Key('app-update-dialog'),
        icon: const Icon(Icons.system_update_alt_outlined),
        title: Text(switch (stage) {
          _UpdateStage.ready => 'Update bereit',
          _UpdateStage.failed => 'Update fehlgeschlagen',
          _ => 'Neue Version verfügbar',
        }),
        content: SizedBox(
          width: 460,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                'MAICENTA ${update.version} ist verfügbar. '
                'Installiert ist ${update.currentVersion}.',
              ),
              if (size.isNotEmpty) ...[
                const SizedBox(height: 4),
                Text('Download: $size', style: theme.textTheme.bodySmall),
              ],
              if (update.releaseNotes.isNotEmpty) ...[
                const SizedBox(height: 16),
                Text(
                  'Neu in dieser Version',
                  style: theme.textTheme.titleSmall,
                ),
                const SizedBox(height: 6),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 220),
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      border: Border.all(color: theme.dividerColor),
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: SingleChildScrollView(
                      padding: const EdgeInsets.all(10),
                      child: SelectableText(
                        update.releaseNotes,
                        key: const Key('app-update-notes'),
                        style: theme.textTheme.bodySmall,
                      ),
                    ),
                  ),
                ),
              ],
              const SizedBox(height: 16),
              ..._stageDetails(theme),
            ],
          ),
        ),
        actions: _actions(),
      ),
    );
  }

  List<Widget> _stageDetails(ThemeData theme) {
    switch (stage) {
      case _UpdateStage.offered when !widget.service.updater.installsInApp:
        return [
          Text(
            'Nach dem Download MAICENTA beenden, die DMG öffnen und die App '
            'in den Ordner „Programme“ ziehen, um die alte Version zu '
            'ersetzen. Das Profil bleibt erhalten.',
            style: theme.textTheme.bodySmall,
          ),
        ];
      case _UpdateStage.offered:
        return [
          Text(
            'MAICENTA startet für das Update neu. Das Profil wird von der '
            'neuen Version übernommen. Bei Beta-Versionen empfiehlt sich '
            'vorher ein Profil-Export (Datei → Import/Export).',
            style: theme.textTheme.bodySmall,
          ),
        ];
      case _UpdateStage.downloading:
        return [
          LinearProgressIndicator(
            key: const Key('app-update-progress'),
            value: progress == 0 ? null : progress / 100,
          ),
          const SizedBox(height: 6),
          Text('Wird heruntergeladen … $progress %'),
        ];
      case _UpdateStage.ready:
        return [
          const Text(
            'Das Update ist heruntergeladen und geprüft. Jetzt neu starten '
            'oder beim nächsten Start von MAICENTA installieren.',
          ),
        ];
      case _UpdateStage.restarting:
        return [
          const LinearProgressIndicator(),
          const SizedBox(height: 6),
          const Text('MAICENTA wird neu gestartet …'),
        ];
      case _UpdateStage.failed:
        return [
          Text(
            failure ?? 'Unbekannter Fehler',
            key: const Key('app-update-failure'),
            style: TextStyle(color: theme.colorScheme.error),
          ),
        ];
    }
  }

  List<Widget> _actions() {
    switch (stage) {
      case _UpdateStage.offered:
        return [
          TextButton(
            key: const Key('app-update-skip'),
            onPressed: skip,
            child: const Text('Diese Version überspringen'),
          ),
          TextButton(
            key: const Key('app-update-later'),
            onPressed: () => Navigator.pop(context),
            child: const Text('Später'),
          ),
          if (widget.service.updater.installsInApp)
            FilledButton(
              key: const Key('app-update-install'),
              onPressed: startDownload,
              child: const Text('Jetzt aktualisieren'),
            )
          else
            FilledButton(
              key: const Key('app-update-open-download'),
              onPressed: openDownload,
              child: const Text('Herunterladen'),
            ),
        ];
      case _UpdateStage.downloading:
      case _UpdateStage.restarting:
        return const [];
      case _UpdateStage.ready:
        return [
          TextButton(
            key: const Key('app-update-next-start'),
            onPressed: () => Navigator.pop(context),
            child: const Text('Beim nächsten Start'),
          ),
          FilledButton(
            key: const Key('app-update-restart'),
            onPressed: restart,
            child: const Text('Jetzt neu starten'),
          ),
        ];
      case _UpdateStage.failed:
        return [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Schließen'),
          ),
          FilledButton(
            key: const Key('app-update-retry'),
            onPressed: widget.service.updater.installsInApp
                ? startDownload
                : openDownload,
            child: const Text('Erneut versuchen'),
          ),
        ];
    }
  }
}
