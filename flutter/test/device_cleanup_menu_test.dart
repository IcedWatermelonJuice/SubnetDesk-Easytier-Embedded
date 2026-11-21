import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/widgets/lan_workspace.dart';

void main() {
  testWidgets('manual removal is offered only in recent connections', (tester) async {
    tester.view.physicalSize = const Size(1200, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final device = WorkspaceDevice(id: 'a' * 64, name: 'Saved computer', address: '10.0.0.2');
    var removed = false;
    await tester.pumpWidget(MaterialApp(home: LanWorkspace(
      nearby: [device], recent: [device], favorites: [device],
      onConnect: (_, __) {}, onFavorite: (_) {}, onRefresh: () {}, onSettings: () {},
      onRemoveRecent: (_) async { removed = true; },
    )));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('workspace-nav-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Recent'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(ValueKey('device-actions-${device.id}')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Remove from recent connections'));
    await tester.pumpAndSettle();
    expect(removed, isTrue);
    await tester.tap(find.text('Nearby'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(ValueKey('device-actions-${device.id}')));
    await tester.pumpAndSettle();
    expect(find.text('Remove from recent connections'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
