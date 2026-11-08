import 'package:flutter/material.dart';

/// Ink well that reports a single click immediately and recognizes a
/// double click on top of it, the way desktop list rows behave.
///
/// Flutter's own `onDoubleTap` postpones every single tap by the double-tap
/// timeout, which makes selection feel sluggish; this widget fires `onTap`
/// at once and calls `onDoubleTap` when a second tap follows quickly.
class DoubleClickInkWell extends StatefulWidget {
  const DoubleClickInkWell({
    super.key,
    required this.onTap,
    required this.onDoubleTap,
    required this.child,
    this.behavior,
  });

  final VoidCallback onTap;
  final VoidCallback onDoubleTap;
  final Widget child;

  /// When set, a plain gesture detector with this hit-test behavior is used
  /// instead of an ink well, for cells without a Material ancestor effect.
  final HitTestBehavior? behavior;

  static const interval = Duration(milliseconds: 350);

  @override
  State<DoubleClickInkWell> createState() => _DoubleClickInkWellState();
}

class _DoubleClickInkWellState extends State<DoubleClickInkWell> {
  DateTime? lastTap;

  void handleTap() {
    final now = DateTime.now();
    final previous = lastTap;
    lastTap = now;
    widget.onTap();
    if (previous != null &&
        now.difference(previous) <= DoubleClickInkWell.interval) {
      lastTap = null;
      widget.onDoubleTap();
    }
  }

  @override
  Widget build(BuildContext context) {
    if (widget.behavior case final behavior?) {
      return GestureDetector(
        behavior: behavior,
        onTap: handleTap,
        child: widget.child,
      );
    }
    return InkWell(onTap: handleTap, child: widget.child);
  }
}
