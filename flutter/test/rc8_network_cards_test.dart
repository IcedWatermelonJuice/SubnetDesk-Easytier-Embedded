import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/widgets/easytier_network_cards.dart';
import 'package:flutter_hbb/common/widgets/easytier_console.dart';
import 'package:flutter_hbb/common/widgets/subnet_ui.dart';

final _relay = <String, dynamic>{
  'id': 'relay', 'name': 'A very long remote computer name for a small mobile screen',
  'address': '10.0.0.2', 'path_kind': 'relay', 'hops': 2,
  'path_latency': 48, 'next_hop_name': 'Hong Kong relay with a long name',
  'next_hop_ip': '10.0.0.3', 'next_hop_latency': 12.3,
  'path_strategy': 'latency_first', 'direct_available': true, 'direct_latency': 90,
  'protocol': 'udp', 'proxy_cidrs': ['192.168.60.0/24'],
};
final _direct = <String, dynamic>{
  'id': 'direct', 'name': 'Direct computer', 'address': '10.0.0.4',
  'path_kind': 'direct', 'hops': 1, 'path_latency': 12.3,
  'direct_available': true, 'direct_latency': 12.3, 'protocol': 'udp',
};

class _Controller extends ChangeNotifier implements EasyTierController {
  final _profile = <String, dynamic>{'id': 'one', 'name': 'A lengthy selected network profile for a phone'};
  @override bool get busy => false;
  @override bool get commandFailed => false;
  @override String get lastCommandError => '';
  @override String get error => '';
  @override String get state => 'running';
  @override String get runningId => 'one';
  @override String get selectedId => 'one';
  @override String get ipv4 => '10.0.0.1';
  @override bool get runningConfigChanged => false;
  @override bool get startingOrStopping => false;
  @override DateTime get queriedAt => DateTime.now();
  @override List<Map<String, dynamic>> get profiles => [_profile];
  @override Map<String, dynamic> get selected => _profile;
  @override List<Map<String, dynamic>> get addressableNodes => [_relay];
  @override String profileName(String id) => _profile['name'] as String;
  @override dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Widget _page({required List<Map<String, dynamic>> nodes, bool stale = false,
  double textScale = 1, ValueChanged<String>? onFill}) => MaterialApp(
  theme: SubnetPalette.apply(ThemeData.light()),
  home: Builder(builder: (context) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
    child: Scaffold(body: SingleChildScrollView(padding: const EdgeInsets.all(16),
      child: EasyTierNetworkCards(nodes: nodes, running: true, stale: stale,
        queriedAt: DateTime.now(), onFill: onFill))),
  )),
);

void main() {
  testWidgets('complete console fits a 320 wide phone with larger text', (tester) async {
    tester.view.physicalSize = const Size(320, 850);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final controller = _Controller();
    addTearDown(controller.dispose);
    await tester.pumpWidget(MaterialApp(theme: SubnetPalette.apply(ThemeData.light()), home: Builder(
      builder: (context) => MediaQuery(data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(1.5)),
        child: Scaffold(body: EasyTierConsole(controller: controller))))));
    await tester.pumpAndSettle();
    expect(find.text('Network'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  for (final width in [320.0, 360.0, 412.0]) {
    testWidgets('phone width $width supports large text, IP action and route sheet', (tester) async {
      tester.view.physicalSize = Size(width, 850);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      String? filled;
      await tester.pumpWidget(_page(nodes: [_relay, _direct], textScale: 2, onFill: (value) => filled = value));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      final link = find.byKey(const ValueKey('easytier-fill-10.0.0.2'));
      await tester.ensureVisible(link);
      await tester.tap(link);
      await tester.pumpAndSettle();
      expect(filled, '10.0.0.2');
      expect(find.byType(DraggableScrollableSheet), findsNothing);
      final card = find.byKey(const ValueKey('easytier-card-relay'));
      await tester.ensureVisible(card);
      await tester.tap(card, warnIfMissed: false);
      await tester.pumpAndSettle();
      // 明确点击详情入口，卡片中心可能在大字体布局下落到内层按钮。
      if (find.byType(DraggableScrollableSheet).evaluate().isEmpty) {
        final details = find.descendant(of: card, matching: find.byTooltip('Details'));
        await tester.ensureVisible(details);
        await tester.tap(details);
        await tester.pumpAndSettle();
      }
      expect(find.byType(DraggableScrollableSheet), findsOneWidget);
      expect(find.text('10.0.0.3'), findsOneWidget);
      expect(tester.takeException(), isNull);
      tester.state<NavigatorState>(find.byType(Navigator)).pop();
      await tester.pumpAndSettle();
    });
  }
  testWidgets('wide cards use two columns and search and path filter agree', (tester) async {
    tester.view.physicalSize = const Size(1100, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(_page(nodes: [_relay, _direct]));
    await tester.pumpAndSettle();
    final relay = tester.getRect(find.byKey(const ValueKey('easytier-card-relay')));
    final direct = tester.getRect(find.byKey(const ValueKey('easytier-card-direct')));
    expect(relay.top, direct.top);
    expect(direct.left, greaterThan(relay.right));
    await tester.tap(find.byKey(const ValueKey('easytier-filter-relay')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('easytier-card-direct')), findsNothing);
    await tester.enterText(find.byKey(const ValueKey('easytier-network-search')), 'missing');
    await tester.pumpAndSettle();
    expect(find.text('No matching devices.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('stale results do not claim a current direct path', (tester) async {
    await tester.pumpWidget(_page(nodes: [_direct], stale: true));
    await tester.pumpAndSettle();
    expect(find.text('Path unknown'), findsOneWidget);
    expect(find.text('0 direct'), findsOneWidget);
    expect(find.text('1 unknown'), findsOneWidget);
    expect(find.text('12.3 ms'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('open route details follow refreshed data and a removed device', (tester) async {
    tester.view.physicalSize = const Size(1100, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(_page(nodes: [_relay]));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Details'));
    await tester.pumpAndSettle();
    expect(find.byType(AlertDialog), findsOneWidget);
    await tester.pumpWidget(_page(nodes: [{..._relay, 'path_latency': 72}]));
    await tester.pumpAndSettle();
    expect(find.descendant(of: find.byType(AlertDialog), matching: find.text('≈ 72.0 ms')), findsOneWidget);
    await tester.pumpWidget(_page(nodes: []));
    await tester.pumpAndSettle();
    expect(find.text('This device is no longer in the current list; its path is unavailable.'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.tap(find.text('Close'));
    await tester.pumpAndSettle();
  });
}
