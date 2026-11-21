import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/widgets/easytier_console.dart';
import 'package:flutter_hbb/common/widgets/lan_workspace.dart';

class _Nodes extends ChangeNotifier implements EasyTierController {
  @override
  bool refreshVisible = false;
  @override
  String get state => 'running';
  @override
  String get error => '';
  @override
  bool get busy => false;
  @override
  List<Map<String, dynamic>> get nodes => [...List.generate(8, (index) => {
    'hostname': 'Node $index',
    'name': 'Node $index',
    'address': '10.88.0.${index + 2}',
  }),
    {'name': 'Missing address', 'address': ''},
    {'name': 'Invalid address', 'address': 'not-an-ip'},
    {'name': 'Unassigned address', 'address': '0.0.0.0'},
  ];
  @override
  List<Map<String, dynamic>> get addressableNodes => nodes.where((node) => easyTierNodeIp(node) != null).toList();

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  testWidgets('home node selection keeps text size and all nodes are scrollable', (tester) async {
    tester.view.physicalSize = const Size(1200, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final nodes = _Nodes();
    await tester.pumpWidget(MaterialApp(home: LanWorkspace(
      nearby: const [], recent: const [], favorites: const [], easyTier: nodes,
      onConnect: (_, __) {}, onFavorite: (_) {}, onRefresh: () {}, onSettings: () {},
    )));
    await tester.pumpAndSettle();
    final refresh = find.byTooltip('Refresh devices');
    final before = tester.getRect(refresh);
    await tester.tap(find.text('EasyTier devices'));
    await tester.pumpAndSettle();
    expect(tester.getRect(refresh).size, before.size);
    expect(find.text('Missing address'), findsNothing);
    expect(find.text('Invalid address'), findsNothing);
    expect(find.text('Unassigned address'), findsNothing);
    expect(find.byIcon(Icons.link_rounded), findsNWidgets(8));
    final scroll = find.byKey(const ValueKey('workspace-home-device-scroll'));
    await tester.drag(scroll, const Offset(0, -1200));
    await tester.pumpAndSettle();
    expect(find.text('Node 7').hitTestable(), findsOneWidget);
    await tester.tap(find.text('View all'));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('workspace-device-list')), findsOneWidget);
    expect(find.text('Device name'), findsOneWidget);
    expect(find.text('Missing address'), findsNothing);
    expect(find.text('Invalid address'), findsNothing);
    expect(find.text('Unassigned address'), findsNothing);
    expect(find.byIcon(Icons.link_rounded), findsWidgets);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    nodes.dispose();
  });
}
