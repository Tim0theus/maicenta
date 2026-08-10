import 'package:flutter/material.dart';

const maicentaPrimaryBlue = Color(0xFF0F6CBD);

@immutable
class MaicentaPalette extends ThemeExtension<MaicentaPalette> {
  const MaicentaPalette({
    required this.window,
    required this.pane,
    required this.chrome,
    required this.subtle,
    required this.input,
    required this.border,
    required this.selected,
    required this.selectedStrong,
    required this.unread,
    required this.mutedText,
    required this.warning,
    required this.dangerTint,
    required this.titleBar,
  });

  final Color window;
  final Color pane;
  final Color chrome;
  final Color subtle;
  final Color input;
  final Color border;
  final Color selected;
  final Color selectedStrong;
  final Color unread;
  final Color mutedText;
  final Color warning;
  final Color dangerTint;
  final Color titleBar;

  static const light = MaicentaPalette(
    window: Color(0xFFFFFFFF),
    pane: Color(0xFFFAFAFA),
    chrome: Color(0xFFF7F7F7),
    subtle: Color(0xFFF3F3F3),
    input: Color(0xFFFFFFFF),
    border: Color(0xFFD1D1D1),
    selected: Color(0xFFDDEAF7),
    selectedStrong: Color(0xFFCFE8FA),
    unread: Color(0xFFF6FAFD),
    mutedText: Color(0xFF666666),
    warning: Color(0xFFFFF4CE),
    dangerTint: Color(0xFFFFE4E1),
    titleBar: Color(0xFF242424),
  );

  static const dark = MaicentaPalette(
    window: Color(0xFF1B1B1B),
    pane: Color(0xFF202020),
    chrome: Color(0xFF252525),
    subtle: Color(0xFF2D2D2D),
    input: Color(0xFF252525),
    border: Color(0xFF484848),
    selected: Color(0xFF243E52),
    selectedStrong: Color(0xFF28506D),
    unread: Color(0xFF202B34),
    mutedText: Color(0xFFB9B9B9),
    warning: Color(0xFF514516),
    dangerTint: Color(0xFF512B2B),
    titleBar: Color(0xFF111111),
  );

  static MaicentaPalette of(BuildContext context) =>
      Theme.of(context).extension<MaicentaPalette>() ?? light;

  @override
  MaicentaPalette copyWith({
    Color? window,
    Color? pane,
    Color? chrome,
    Color? subtle,
    Color? input,
    Color? border,
    Color? selected,
    Color? selectedStrong,
    Color? unread,
    Color? mutedText,
    Color? warning,
    Color? dangerTint,
    Color? titleBar,
  }) {
    return MaicentaPalette(
      window: window ?? this.window,
      pane: pane ?? this.pane,
      chrome: chrome ?? this.chrome,
      subtle: subtle ?? this.subtle,
      input: input ?? this.input,
      border: border ?? this.border,
      selected: selected ?? this.selected,
      selectedStrong: selectedStrong ?? this.selectedStrong,
      unread: unread ?? this.unread,
      mutedText: mutedText ?? this.mutedText,
      warning: warning ?? this.warning,
      dangerTint: dangerTint ?? this.dangerTint,
      titleBar: titleBar ?? this.titleBar,
    );
  }

  @override
  MaicentaPalette lerp(covariant MaicentaPalette? other, double t) {
    if (other == null) return this;
    return MaicentaPalette(
      window: Color.lerp(window, other.window, t)!,
      pane: Color.lerp(pane, other.pane, t)!,
      chrome: Color.lerp(chrome, other.chrome, t)!,
      subtle: Color.lerp(subtle, other.subtle, t)!,
      input: Color.lerp(input, other.input, t)!,
      border: Color.lerp(border, other.border, t)!,
      selected: Color.lerp(selected, other.selected, t)!,
      selectedStrong: Color.lerp(selectedStrong, other.selectedStrong, t)!,
      unread: Color.lerp(unread, other.unread, t)!,
      mutedText: Color.lerp(mutedText, other.mutedText, t)!,
      warning: Color.lerp(warning, other.warning, t)!,
      dangerTint: Color.lerp(dangerTint, other.dangerTint, t)!,
      titleBar: Color.lerp(titleBar, other.titleBar, t)!,
    );
  }
}

ThemeData buildMaicentaTheme(Brightness brightness) {
  final dark = brightness == Brightness.dark;
  final palette = dark ? MaicentaPalette.dark : MaicentaPalette.light;
  final scheme = ColorScheme.fromSeed(
    seedColor: maicentaPrimaryBlue,
    brightness: brightness,
    primary: dark ? const Color(0xFF65AEEB) : maicentaPrimaryBlue,
    surface: palette.window,
  );
  final base = ThemeData(
    brightness: brightness,
    colorScheme: scheme,
    scaffoldBackgroundColor: palette.window,
    canvasColor: palette.pane,
    cardColor: palette.pane,
    dialogTheme: DialogThemeData(backgroundColor: palette.pane),
    popupMenuTheme: PopupMenuThemeData(color: palette.pane),
    fontFamily: 'Segoe UI',
    visualDensity: VisualDensity.compact,
    dividerColor: palette.border,
    tooltipTheme: const TooltipThemeData(
      waitDuration: Duration(milliseconds: 350),
    ),
    inputDecorationTheme: InputDecorationTheme(
      isDense: true,
      filled: true,
      fillColor: palette.input,
      border: const OutlineInputBorder(borderSide: BorderSide.none),
    ),
    extensions: [palette],
  );
  return base.copyWith(
    textSelectionTheme: TextSelectionThemeData(
      cursorColor: scheme.primary,
      selectionColor: scheme.primary.withValues(alpha: 0.35),
    ),
  );
}
