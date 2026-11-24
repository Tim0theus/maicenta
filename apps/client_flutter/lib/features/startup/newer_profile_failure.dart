/// The Rust core refused a profile that a newer MAICENTA version has already
/// migrated (`ApplicationError::ProfileNewerThanApp`). Nothing was changed;
/// installing the current version opens it again.
class NewerProfileFailure {
  const NewerProfileFailure({
    required this.profileSchema,
    required this.supportedSchema,
  });

  final int profileSchema;
  final int supportedSchema;

  /// Matches the core's error text; keep in sync with the `Display` of
  /// `ProfileNewerThanApp` in `core/application`.
  static final _pattern = RegExp(
    r'profile schema (\d+) was written by a newer MAICENTA version; '
    r'this version supports up to schema (\d+)',
  );

  static NewerProfileFailure? tryParse(String message) {
    final match = _pattern.firstMatch(message);
    if (match == null) return null;
    return NewerProfileFailure(
      profileSchema: int.parse(match.group(1)!),
      supportedSchema: int.parse(match.group(2)!),
    );
  }

  /// Title for the startup screen and the import error.
  static const title = 'Profil stammt aus einer neueren Version';

  /// Explanation shown when the profile on this device cannot be opened.
  static const startupExplanation =
      'Dieses Profil wurde zuletzt mit einer neueren MAICENTA-Version '
      'geöffnet und kann von dieser Version nicht gelesen werden. Deine '
      'Daten sind unverändert. Installiere die aktuelle Version, um '
      'weiterzuarbeiten.';

  /// Explanation shown when an imported backup is too new.
  static const importExplanation =
      'Diese Sicherung wurde mit einer neueren MAICENTA-Version erstellt und '
      'kann von dieser Version nicht gelesen werden. Dein aktuelles Profil '
      'ist unverändert. Installiere die aktuelle Version und importiere die '
      'Sicherung dann erneut.';

  /// One line for bug reports.
  String get details =>
      'Profilschema $profileSchema, diese Version unterstützt bis '
      'Schema $supportedSchema.';
}
