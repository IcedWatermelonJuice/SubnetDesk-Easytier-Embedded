import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/widgets/easytier_console.dart';

class _Profiles extends ChangeNotifier implements EasyTierController {
  _Profiles(this.latencyFirst);
  final bool latencyFirst;
  Map<String, Object?>? saved;
  @override
  bool refreshVisible = false;
  @override
  bool get available => true;
  @override
  bool get busy => false;
  @override
  bool get startingOrStopping => false;
  @override
  bool get commandFailed => false;
  @override
  String get lastCommandError => '';
  @override
  String get error => '';
  @override
  String get state => 'stopped';
  @override
  String get selectedId => 'test';
  @override
  String get runningId => '';
  @override
  String get ipv4 => '';
  @override
  DateTime? get queriedAt => null;
  @override
  bool get runningConfigChanged => false;
  @override
  List<Map<String, dynamic>> get profiles => [{'id': 'test', 'name': 'sample'}];
  @override
  Map<String, dynamic>? get selected => profiles.first;
  @override
  String profileName(String id) => 'sample';
  @override
  List<Map<String, dynamic>> get nodes => [];
  @override
  List<Map<String, dynamic>> get addressableNodes => [];
  @override
  List<Map<String, dynamic>> get routes => [];
  @override
  Future<Map<String, dynamic>?> readProfile(String id) async => {
    'id': id,
    'name': 'sample',
    'toml': '[flags]\nlatency_first = $latencyFirst\n',
    'fields': {'dhcp': true, 'latency_first': latencyFirst},
  };
  @override
  Future<bool> command(String action, [Map<String, Object?> arguments = const {}]) async {
    if (action == 'save') saved = arguments;
    return true;
  }
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  for (final width in [320.0, 1200.0]) {
    testWidgets('方案延迟优先开关在 $width 宽度回显并保存', (tester) async {
      tester.view.physicalSize = Size(width, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final controller = _Profiles(false);
      await tester.pumpWidget(MaterialApp(home: Scaffold(body: EasyTierConsole(controller: controller))));
      if (width < 600) {
        await tester.tap(find.byTooltip('Profile actions'));
        await tester.pumpAndSettle();
      }
      await tester.tap(find.text(width < 600 ? 'Edit profile' : 'Edit'));
      await tester.pumpAndSettle();
      final toggle = find.byKey(const ValueKey('easytier-latency-first'));
      await tester.ensureVisible(toggle);
      expect(tester.widget<SwitchListTile>(toggle).value, isFalse);
      await tester.tap(toggle);
      await tester.pumpAndSettle();
      expect(tester.widget<SwitchListTile>(toggle).value, isTrue);
      await tester.ensureVisible(find.text('Save'));
      await tester.tap(find.text('Save'));
      await tester.pumpAndSettle();
      expect((controller.saved?['fields'] as Map)['latency_first'], isTrue);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      controller.dispose();
    });
  }
}
