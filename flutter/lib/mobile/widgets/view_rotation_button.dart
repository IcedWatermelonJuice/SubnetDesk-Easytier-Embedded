import 'package:flutter/material.dart';

/// 手机远控的本地画面旋转入口；角度和持久化由会话模型管理。
class MobileViewRotationButton extends StatelessWidget {
  const MobileViewRotationButton({
    super.key,
    required this.angle,
    required this.enabled,
    required this.tooltip,
    required this.onSelected,
  });

  final int angle;
  final bool enabled;
  final String tooltip;
  final ValueChanged<int> onSelected;

  @override
  Widget build(BuildContext context) => PopupMenuButton<int>(
    tooltip: '$tooltip ($angle°)',
    enabled: enabled,
    initialValue: angle,
    icon: const Icon(Icons.rotate_right, color: Colors.white),
    onSelected: onSelected,
    itemBuilder: (_) => [
      for (final value in const [0, 90, 180, 270])
        CheckedPopupMenuItem<int>(
          value: value,
          checked: angle == value,
          child: Text('$value°'),
        ),
    ],
  );
}
