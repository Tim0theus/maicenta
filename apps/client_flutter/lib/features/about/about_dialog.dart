import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:package_info_plus/package_info_plus.dart';

import '../../build_version.dart';

/// Public project links shown in the about dialog.
const maicentaRepositoryUrl = 'https://github.com/Tim0theus/maicenta';
const maicentaReleasesUrl = '$maicentaRepositoryUrl/releases';
const maicentaIssuesUrl = '$maicentaRepositoryUrl/issues/new/choose';
const maicentaSecurityUrl = '$maicentaRepositoryUrl/security/advisories/new';

/// Version and environment details that a bug report should contain.
class MaicentaBuildInfo {
  const MaicentaBuildInfo({
    required this.version,
    required this.buildNumber,
    required this.platform,
  });

  final String version;
  final String buildNumber;
  final String platform;

  /// `0.1.0-beta.1 (2)`; the build number is omitted when unknown.
  String get versionLabel =>
      buildNumber.isEmpty ? version : '$version ($buildNumber)';

  bool get isPreRelease => version.contains('-');

  /// One line for the clipboard and issue templates.
  String get diagnosticsLine => 'MAICENTA $versionLabel · $platform';

  /// The version comes from [maicentaVersion] so pre-release suffixes survive
  /// on every platform; only the build number is read from the bundle.
  static Future<MaicentaBuildInfo> load() async {
    var buildNumber = '';
    try {
      buildNumber = (await PackageInfo.fromPlatform()).buildNumber;
    } on Object {
      // The dialog still shows the version when platform info is unavailable.
    }
    return MaicentaBuildInfo(
      version: maicentaVersion,
      buildNumber: buildNumber,
      platform: currentPlatformLabel(),
    );
  }

  static String currentPlatformLabel() {
    if (kIsWeb) return 'Web';
    final os = switch (defaultTargetPlatform) {
      TargetPlatform.macOS => 'macOS',
      TargetPlatform.windows => 'Windows',
      TargetPlatform.linux => 'Linux',
      TargetPlatform.android => 'Android',
      TargetPlatform.iOS => 'iOS',
      TargetPlatform.fuchsia => 'Fuchsia',
    };
    return '$os ${Platform.operatingSystemVersion}'.trim();
  }
}

/// Registers the Rust third-party notices with Flutter's license page.
///
/// The asset is produced by `cargo about generate` (see `about.toml`) and
/// contains one block per license text with the crates it covers.
void registerRustLicenses() {
  LicenseRegistry.addLicense(() async* {
    final String raw;
    try {
      raw = await rootBundle.loadString('assets/licenses/rust-third-party.txt');
    } on Object {
      return;
    }
    for (final entry in parseRustLicenseNotice(raw)) {
      yield LicenseEntryWithLineBreaks(entry.packages, entry.text);
    }
  });
}

class RustLicenseBlock {
  const RustLicenseBlock({required this.packages, required this.text});

  final List<String> packages;
  final String text;
}

/// Splits the generated notice into per-license blocks.
///
/// Block format written by `about.hbs`:
/// ```
/// ==== LICENSE: MIT
/// PACKAGES: crate-a 1.0.0, crate-b 2.3.1
///
/// <license text>
/// ```
List<RustLicenseBlock> parseRustLicenseNotice(String raw) {
  final blocks = <RustLicenseBlock>[];
  for (final chunk in raw.split('==== LICENSE: ').skip(1)) {
    final lines = chunk.split('\n');
    if (lines.isEmpty) continue;
    final licenseName = lines.first.trim();
    var index = 1;
    final packages = <String>[];
    if (index < lines.length && lines[index].startsWith('PACKAGES: ')) {
      packages.addAll(
        lines[index]
            .substring('PACKAGES: '.length)
            .split(', ')
            .map((package) => package.trim())
            .where((package) => package.isNotEmpty),
      );
      index += 1;
    }
    final text = lines.skip(index).join('\n').trim();
    if (text.isEmpty) continue;
    blocks.add(
      RustLicenseBlock(
        packages: packages.isEmpty ? ['Rust core ($licenseName)'] : packages,
        text: text,
      ),
    );
  }
  return blocks;
}

/// Shows version, platform, project links, and the open-source licenses.
Future<void> showMaicentaAboutDialog(
  BuildContext context, {
  Future<MaicentaBuildInfo> Function()? loadBuildInfo,
}) {
  return showDialog<void>(
    context: context,
    builder: (dialogContext) => FutureBuilder<MaicentaBuildInfo>(
      future: (loadBuildInfo ?? MaicentaBuildInfo.load)(),
      builder: (context, snapshot) {
        final info = snapshot.data;
        return AlertDialog(
          key: const Key('about-dialog'),
          title: Row(
            children: [
              Image.asset(
                'assets/branding/maicenta-symbol.png',
                width: 28,
                height: 28,
              ),
              const SizedBox(width: 10),
              const Text('Über MAICENTA'),
            ],
          ),
          content: SizedBox(
            width: 460,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (info == null)
                    const Padding(
                      padding: EdgeInsets.symmetric(vertical: 8),
                      child: LinearProgressIndicator(),
                    )
                  else ...[
                    Wrap(
                      spacing: 8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Text(
                          'Version ${info.versionLabel}',
                          key: const Key('about-version'),
                          style: const TextStyle(fontWeight: FontWeight.w600),
                        ),
                        if (info.isPreRelease)
                          Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 6,
                              vertical: 1,
                            ),
                            decoration: BoxDecoration(
                              color: Theme.of(context).colorScheme.primary,
                              borderRadius: BorderRadius.circular(3),
                            ),
                            child: Text(
                              'Beta',
                              style: TextStyle(
                                fontSize: 11,
                                color: Theme.of(context).colorScheme.onPrimary,
                              ),
                            ),
                          ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    Text(
                      info.platform,
                      key: const Key('about-platform'),
                      style: const TextStyle(fontSize: 12),
                    ),
                    const SizedBox(height: 10),
                    const Text(
                      'Lokaler, modularer Arbeitsplatz für E-Mail, Kalender, '
                      'Aufgaben und Kontakte. Lizenz: Mozilla Public License 2.0.',
                      style: TextStyle(fontSize: 12),
                    ),
                    const SizedBox(height: 10),
                    Text(
                      'Bitte gib in Fehlerberichten diese Zeile an:',
                      style: TextStyle(
                        fontSize: 12,
                        color: Theme.of(context).colorScheme.onSurfaceVariant,
                      ),
                    ),
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        Expanded(
                          child: SelectableText(
                            info.diagnosticsLine,
                            key: const Key('about-diagnostics'),
                            style: const TextStyle(
                              fontSize: 12,
                              fontFamily: 'monospace',
                            ),
                          ),
                        ),
                        IconButton(
                          key: const Key('about-copy-diagnostics'),
                          tooltip: 'In die Zwischenablage kopieren',
                          icon: const Icon(Icons.copy, size: 16),
                          onPressed: () => Clipboard.setData(
                            ClipboardData(text: info.diagnosticsLine),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    const _AboutLink(
                      label: 'Quellcode',
                      url: maicentaRepositoryUrl,
                    ),
                    const _AboutLink(
                      label: 'Versionen und Hinweise',
                      url: maicentaReleasesUrl,
                    ),
                    const _AboutLink(
                      label: 'Fehler melden oder Idee vorschlagen',
                      url: maicentaIssuesUrl,
                    ),
                    const _AboutLink(
                      label: 'Sicherheitslücke vertraulich melden',
                      url: maicentaSecurityUrl,
                    ),
                  ],
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
              key: const Key('about-licenses'),
              onPressed: () => showLicensePage(
                context: context,
                applicationName: 'MAICENTA',
                applicationVersion: info?.versionLabel,
                applicationLegalese: 'Mozilla Public License 2.0',
              ),
              child: const Text('Open-Source-Lizenzen'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(dialogContext),
              child: const Text('Schließen'),
            ),
          ],
        );
      },
    ),
  );
}

class _AboutLink extends StatelessWidget {
  const _AboutLink({required this.label, required this.url});

  final String label;
  final String url;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label, style: const TextStyle(fontSize: 12)),
          SelectableText(
            url,
            style: TextStyle(
              fontSize: 12,
              color: Theme.of(context).colorScheme.primary,
            ),
          ),
        ],
      ),
    );
  }
}
