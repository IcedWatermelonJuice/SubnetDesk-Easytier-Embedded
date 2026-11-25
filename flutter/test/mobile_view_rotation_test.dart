import 'package:flutter/material.dart';
import 'package:flutter_hbb/mobile/widgets/view_rotation_button.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('窄屏工具栏可选择并恢复全部画面角度', (tester) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    var angle = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: StatefulBuilder(
          builder: (context, setState) => Scaffold(
            bottomNavigationBar: Row(
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: [
                        MobileViewRotationButton(
                          angle: angle,
                          enabled: true,
                          tooltip: '旋转画面',
                          onSelected: (value) => setState(() => angle = value),
                        ),
                        for (var i = 0; i < 8; i++)
                          IconButton(
                            onPressed: () {},
                            icon: const Icon(Icons.mouse),
                          ),
                      ],
                    ),
                  ),
                ),
                IconButton(
                  onPressed: () {},
                  icon: const Icon(Icons.expand_more),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    for (final value in [90, 180, 270, 0]) {
      await tester.tap(find.byType(MobileViewRotationButton));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byWidgetPredicate(
          (widget) =>
              widget is CheckedPopupMenuItem<int> && widget.value == value,
        ),
      );
      await tester.pumpAndSettle();
      expect(angle, value);
      expect(find.byTooltip('旋转画面 ($value°)'), findsOneWidget);
      expect(tester.takeException(), isNull);
    }
    await tester.drag(
      find.byType(SingleChildScrollView),
      const Offset(-350, 0),
    );
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('尚无可旋转单屏时不能打开菜单', (tester) async {
    var called = false;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MobileViewRotationButton(
            angle: 0,
            enabled: false,
            tooltip: '旋转画面',
            onSelected: (_) => called = true,
          ),
        ),
      ),
    );
    await tester.tap(find.byType(MobileViewRotationButton));
    await tester.pumpAndSettle();
    expect(find.text('90°'), findsNothing);
    expect(called, isFalse);
  });
}
