import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:path/path.dart' as path;

import 'subnet_ui.dart';

typedef _Entry = Map<String, dynamic>;

String _text(BuildContext context, String zh, String en) =>
    subnetText(context, zh, en);

String _string(Object? value) => value is String ? value : '';
List<_Entry> _entries(Object? value) =>
    value is List ? value.whereType<_Entry>().toList() : const [];

const easyTierDeviceSource = 4;

class EasyTierController extends ChangeNotifier {
  _Entry _status = const {};
  String? _lastSnapshot;
  late final Timer _poller;
  bool _disposed = false;
  int _ticks = 0;
  int _sequence = 0;
  String? _requestId;
  Completer<bool>? _completion;
  String lastCommandError = '';
  bool refreshVisible = false;

  EasyTierController() {
    readStatus();
    _poller = Timer.periodic(const Duration(seconds: 1), (_) {
      readStatus();
      if (++_ticks % 5 == 0 && refreshVisible && state == 'running' && !busy) {
        unawaited(_refresh());
      }
    });
  }

  Future<void> _refresh() async {
    try {
      await bind.mainSetLocalOption(
        key: 'easytier-command',
        value: '{"action":"refresh"}',
      );
    } catch (_) {
      // 后台刷新错误由引擎状态报告。
    }
  }
  bool busy = false;
  bool commandFailed = false;

  bool get available => _status['available'] == true;
  List<_Entry> get profiles =>
      _entries(_status['profiles']).where((p) => _string(p['id']).isNotEmpty).toList();
  String get selectedId => _string(_status['selected_id']);
  String get runningId => _string(_status['running_id']);
  String get state => _string(_status['state']);
  String get ipv4 => _string(_status['ipv4']);
  String get error => _string(_status['error']);
  bool get startingOrStopping => state == 'starting' || state == 'stopping';
  bool get remoteOnly => _status['remote_only'] == true;
  bool get runningConfigChanged => _status['running_config_changed'] == true;
  DateTime? get queriedAt {
    final timestamp = _status['queried_at'];
    return timestamp is int && timestamp > 0
        ? DateTime.fromMillisecondsSinceEpoch(timestamp)
        : null;
  }
  List<_Entry> get nodes =>
      state == 'running' ? _entries(_status['nodes']) : const [];
  List<_Entry> get routes =>
      state == 'running' ? _entries(_status['routes']) : const [];

  _Entry? get selected =>
      profiles.where((p) => p['id'] == selectedId).firstOrNull;

  String profileName(String id) =>
      _string(profiles.where((p) => p['id'] == id).firstOrNull?['name']);

  void readStatus() {
    if (_disposed) return;
    try {
      final raw = bind.mainGetLocalOption(key: 'easytier-status');
      if (raw == _lastSnapshot) return;
      final decoded = jsonDecode(raw);
      _status = decoded is _Entry ? decoded : const {};
      _lastSnapshot = raw;
      final completion = _completion;
      if (completion != null &&
          !completion.isCompleted &&
          _status['completed_request_id'] == _requestId) {
        lastCommandError = _string(_status['command_error']);
        completion.complete(lastCommandError.isEmpty);
      }
    } catch (_) {
      if (_status.isEmpty && _lastSnapshot == null) return;
      _status = const {};
      _lastSnapshot = null;
    }
    notifyListeners();
  }

  _Entry? readProfile(String id) {
    try {
      final profile = jsonDecode(bind.mainGetLocalOption(key: 'easytier-profile:$id'));
      if (profile is _Entry && profile['id'] == id) return profile;
    } catch (_) {
      // 配置可能含密钥，不记录底层返回值。
    }
    return null;
  }

  Future<bool> command(String action, [Map<String, Object?> arguments = const {}]) async {
    final canDisable = action == 'stop' || (action == 'set_remote_only' && arguments['enabled'] == false);
    if (busy || (!available && !canDisable) || _disposed) return false;
    busy = true;
    commandFailed = false;
    lastCommandError = '';
    final requestId = '${DateTime.now().microsecondsSinceEpoch}-${++_sequence}';
    final completion = Completer<bool>();
    _requestId = requestId;
    _completion = completion;
    notifyListeners();
    try {
      await bind.mainSetLocalOption(
        key: 'easytier-command',
        value: jsonEncode({'action': action, ...arguments, 'request_id': requestId}),
      );
      if (_disposed) return false;
      readStatus();
      final ok = await completion.future.timeout(
        const Duration(seconds: 45),
        onTimeout: () => false,
      );
      if (_disposed) return false;
      commandFailed = !ok;
      return ok;
    } catch (_) {
      if (!_disposed) commandFailed = true;
      return false;
    } finally {
      _completion = null;
      _requestId = null;
      busy = false;
      if (!_disposed) notifyListeners();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _poller.cancel();
    final completion = _completion;
    if (completion != null && !completion.isCompleted) completion.complete(false);
    super.dispose();
  }
}

Future<void> startSelectedEasyTier(
  BuildContext context,
  EasyTierController controller, {
  required VoidCallback openConsole,
}) async {
  final selected = controller.selected;
  if (selected == null) {
    openConsole();
    return;
  }
  if (controller.busy || controller.startingOrStopping) return;
  final target = _string(selected['id']);
  if (controller.runningId.isNotEmpty) {
    final restarting = controller.runningId == target;
    if (restarting && !controller.runningConfigChanged) return;
    final currentName = controller.profileName(controller.runningId);
    final targetName = _string(selected['name']);
    final accepted = await _confirm(
      context,
      restarting
          ? _text(context, '重启组网', 'Restart network')
          : _text(context, '切换组网方案', 'Switch network profile'),
      restarting
          ? _text(context, '重启「$targetName」以应用更改。', 'Restart “$targetName” to apply changes.')
          : _text(context, '停止「$currentName」，启动「$targetName」。', 'Stop “$currentName” and start “$targetName”.'),
    );
    if (!accepted || !context.mounted || controller.selectedId != target) return;
    if (restarting && !await controller.command('stop')) return;
    if (!context.mounted || controller.selectedId != target) return;
  }
  final ok = await controller.command('start');
  if (!ok && context.mounted) openConsole();
}

Future<bool> _confirm(BuildContext context, String title, String body) async =>
    await showDialog<bool>(
      context: context,
      builder: (dialog) => AlertDialog(
        title: Text(title),
        content: Text(body),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialog, false),
            child: Text(_text(context, '取消', 'Cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(dialog, true),
            child: Text(_text(context, '确认', 'Confirm')),
          ),
        ],
      ),
    ) ??
    false;

class EasyTierConsole extends StatefulWidget {
  const EasyTierConsole({super.key, required this.controller});

  final EasyTierController controller;

  @override
  State<EasyTierConsole> createState() => _EasyTierConsoleState();
}

class _EasyTierConsoleState extends State<EasyTierConsole> {
  EasyTierController get controller => widget.controller;
  bool _showRoutes = false;

  String t(String zh, String en) => _text(context, zh, en);

  Future<void> _edit([String? id]) async {
    final profile = id == null ? null : controller.readProfile(id);
    if (id != null && profile == null) {
      _notice(t('无法读取方案。', 'Could not read the profile.'));
      return;
    }
    await showDialog<void>(
      context: context,
      builder: (_) => _ProfileEditor(controller: controller, profile: profile),
    );
  }

  void _notice(String text) => ScaffoldMessenger.of(context)
      .showSnackBar(SnackBar(content: Text(text)));

  Future<String?> _chooseName(String title, String initial) => showDialog<String>(
    context: context,
    builder: (_) => _ProfileNameDialog(title: title, initial: initial),
  );

  Future<void> _copy(_Entry profile) async {
    final name = _string(profile['name']);
    final result = await _chooseName(
      t('复制方案', 'Duplicate profile'),
      t('$name 副本', '$name copy'),
    );
    if (mounted && result != null) {
      await controller.command('duplicate', {'id': profile['id'], 'name': result});
    }
  }

  Future<void> _rename(_Entry profile) async {
    final result = await _chooseName(t('重命名方案', 'Rename profile'), _string(profile['name']));
    if (mounted && result != null) {
      await controller.command('save', {'id': profile['id'], 'name': result});
    }
  }

  Future<void> _delete(_Entry profile) async {
    final id = _string(profile['id']);
    final name = _string(profile['name']);
    if (!await _confirm(
      context,
      t('删除方案', 'Delete profile'),
      t('删除「$name」？', 'Delete “$name”?'),
    ) || !mounted || controller.runningId == id) {
      return;
    }
    await controller.command('delete', {'id': id});
  }

  Future<void> _import() async {
    try {
      final result = await FilePicker.pickFiles(
        type: FileType.custom,
        allowedExtensions: ['toml'],
      );
      if (!mounted || result == null) return;
      final file = result.files.single.path;
      if (file == null) return;
      final name = await _chooseName(
        t('导入方案', 'Import profile'),
        path.basenameWithoutExtension(file),
      );
      if (mounted && name != null) {
        await controller.command('import', {'path': file, 'name': name});
      }
    } catch (_) {
      if (mounted) _notice(t('无法选择 TOML 文件。', 'Could not select a TOML file.'));
    }
  }

  Future<void> _export(_Entry profile) async {
    if (!await _confirm(
      context,
      t('导出配置', 'Export configuration'),
      t('TOML 文件可能包含网络密钥。请妥善保管。', 'The TOML file may contain a network secret. Keep it private.'),
    ) || !mounted) {
      return;
    }
    try {
      final file = await FilePicker.saveFile(
        fileName: 'config.toml',
        type: FileType.custom,
        allowedExtensions: ['toml'],
      );
      if (mounted && file != null) {
        final ok = await controller.command('export', {'path': file, 'id': profile['id']});
        if (ok && mounted) _notice(t('已导出配置。', 'Configuration exported.'));
      }
    } catch (_) {
      if (mounted) _notice(t('无法选择保存位置。', 'Could not select an export destination.'));
    }
  }

  @override
  Widget build(BuildContext context) {
    final selected = controller.selected;
    final selectedId = selected?['id'];
    final canStart = !controller.busy && !controller.startingOrStopping &&
        selected != null &&
        (controller.runningId != selectedId || controller.runningConfigChanged);
    final canStop = !controller.busy && controller.state == 'running';
    final title = Theme.of(context).textTheme;
    final queriedAt = controller.queriedAt;
    final queryTime = queriedAt == null
        ? null
        : MaterialLocalizations.of(context).formatTimeOfDay(
            TimeOfDay.fromDateTime(queriedAt),
          );
    final error = controller.lastCommandError.isNotEmpty
        ? controller.lastCommandError
        : controller.error;
    final stateLabel = switch (controller.state) {
      'starting' => t('正在启动', 'Starting'),
      'running' => t('运行中', 'Running'),
      'stopping' => t('正在停止', 'Stopping'),
      'failed' => t('启动或运行失败', 'Failed'),
      _ => t('已停止', 'Stopped'),
    };
    final runningName = controller.profileName(controller.runningId);
    final buttonLabel = controller.runningId.isEmpty
        ? t('启动', 'Start')
        : controller.runningId == selectedId
        ? t('重启', 'Restart')
        : t('切换运行方案', 'Switch running profile');

    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1040),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(32, 12, 32, 32),
          children: [
            Text(t('组网', 'Network'), style: title.headlineSmall),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                Chip(label: Text(stateLabel)),
                if (runningName.isNotEmpty)
                  Chip(label: Text(t('运行方案 · $runningName', 'Running · $runningName'))),
                if (controller.ipv4.isNotEmpty)
                  Chip(label: Text(controller.ipv4)),
                if (queryTime != null)
                  Chip(label: Text(t('查询 · $queryTime', 'Queried · $queryTime'))),
                if (controller.runningConfigChanged)
                  Chip(label: Text(t('配置已更改', 'Configuration changed'))),
              ],
            ),
            if (error.isNotEmpty || controller.commandFailed)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(
                  error.isNotEmpty ? error : t('EasyTier 操作失败。', 'EasyTier action failed.'),
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            const SizedBox(height: 14),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                SizedBox(
                  width: 260,
                  child: InputDecorator(
                    decoration: InputDecoration(
                      labelText: t('选中的方案', 'Selected profile'),
                      border: const OutlineInputBorder(),
                    ),
                    child: DropdownButtonHideUnderline(
                      child: DropdownButton<String>(
                        isExpanded: true,
                        isDense: true,
                        value: selectedId is String ? selectedId : null,
                        hint: Text(t('请选择方案', 'Select a profile')),
                        items: [
                          for (final profile in controller.profiles)
                            DropdownMenuItem(
                              value: _string(profile['id']),
                              child: Text(_string(profile['name']), overflow: TextOverflow.ellipsis),
                            ),
                        ],
                        onChanged: controller.busy
                            ? null
                            : (id) {
                                if (id != null) controller.command('select', {'id': id});
                              },
                      ),
                    ),
                  ),
                ),
                FilledButton(
                  onPressed: controller.busy ? null : () => _edit(),
                  child: Text(t('新建', 'New')),
                ),
                OutlinedButton(
                  onPressed: controller.busy ? null : _import,
                  child: Text(t('导入 TOML', 'Import TOML')),
                ),
                if (selected != null) ...[
                  OutlinedButton(
                    onPressed: controller.busy ? null : () => _edit(_string(selectedId)),
                    child: Text(t('编辑', 'Edit')),
                  ),
                  PopupMenuButton<String>(
                    enabled: !controller.busy,
                    tooltip: t('方案操作', 'Profile actions'),
                    onSelected: (action) {
                      switch (action) {
                        case 'rename':
                          _rename(selected);
                        case 'copy':
                          _copy(selected);
                        case 'delete':
                          _delete(selected);
                        case 'export':
                          _export(selected);
                      }
                    },
                    itemBuilder: (_) => [
                      PopupMenuItem(value: 'rename', child: Text(t('重命名', 'Rename'))),
                      PopupMenuItem(value: 'copy', child: Text(t('复制', 'Duplicate'))),
                      PopupMenuItem(value: 'export', child: Text(t('导出 TOML', 'Export TOML'))),
                      PopupMenuItem(
                        value: 'delete',
                        enabled: controller.runningId != selectedId,
                        child: Text(t('删除', 'Delete')),
                      ),
                    ],
                  ),
                ],
              ],
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                FilledButton(
                  onPressed: canStart
                      ? () => startSelectedEasyTier(context, controller, openConsole: () {})
                      : null,
                  child: Text(buttonLabel),
                ),
                OutlinedButton(
                  onPressed: canStop ? () => controller.command('stop') : null,
                  child: Text(t('停止', 'Stop')),
                ),
              ],
            ),
            const SizedBox(height: 24),
            Wrap(
              spacing: 10,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                ChoiceChip(
                  label: Text(t('节点', 'Nodes')),
                  selected: !_showRoutes,
                  onSelected: (_) => setState(() => _showRoutes = false),
                ),
                ChoiceChip(
                  label: Text(t('路由', 'Routes')),
                  selected: _showRoutes,
                  onSelected: (_) => setState(() => _showRoutes = true),
                ),
                IconButton(
                  tooltip: t('刷新', 'Refresh'),
                  onPressed: controller.busy || controller.state != 'running'
                      ? null
                      : () => controller.command('refresh'),
                  icon: const Icon(Icons.refresh),
                ),
              ],
            ),
            const Divider(),
            if (_showRoutes) ..._routeRows() else EasyTierNodeList(controller: controller),
          ],
        ),
      ),
    );
  }

  List<Widget> _routeRows() {
    if (controller.state != 'running' || controller.routes.isEmpty) {
      return [
        Text(controller.state != 'running'
            ? _networkStateMessage(context, controller.state)
            : controller.error.isNotEmpty
            ? t('路由查询失败。', 'Could not read routes.')
            : t('没有路由。', 'No routes.')),
      ];
    }
    return [
      for (final route in controller.routes)
        ListTile(
          contentPadding: EdgeInsets.zero,
          title: Text(_string(route['destination'])),
          subtitle: Text([
            if (_string(route['next_hop']).isNotEmpty)
              t('下一跳 ${route['next_hop']}', 'Next hop ${route['next_hop']}'),
            if (route['hops'] != null)
              t('${route['hops']} 跳', '${route['hops']} hops'),
            if (route['latency'] != null) '${route['latency']} ms',
          ].join(' · ')),
        ),
    ];
  }
}

String _networkStateMessage(BuildContext context, String state) => switch (state) {
  'starting' => _text(context, '正在启动组网…', 'Starting the network…'),
  'stopping' => _text(context, '正在停止组网…', 'Stopping the network…'),
  'failed' => _text(context, '组网运行失败。', 'The network failed.'),
  _ => _text(context, '尚未启动组网。', 'The network is stopped.'),
};

class EasyTierNodeList extends StatelessWidget {
  const EasyTierNodeList({super.key, required this.controller, this.onFill, this.maxItems});

  final EasyTierController controller;
  final ValueChanged<String>? onFill;
  final int? maxItems;

  @override
  Widget build(BuildContext context) {
    final nodes = controller.nodes;
    if (controller.state != 'running' || nodes.isEmpty) {
      return Text(controller.state != 'running'
          ? _networkStateMessage(context, controller.state)
          : controller.error.isNotEmpty
          ? _text(context, '节点查询失败。', 'Could not read nodes.')
          : _text(context, '当前没有节点。', 'No nodes in the current network.'));
    }
    return Column(
      children: [
        for (final node in nodes.take(maxItems ?? nodes.length)) _row(context, node),
      ],
    );
  }

  Widget _row(BuildContext context, _Entry node) {
    final ip = InternetAddress.tryParse(_string(node['address']).trim());
    final valid = ip != null && !ip.isLoopback &&
        ip.address != '0.0.0.0' && ip.address != '::';
    final name = _string(node['name']);
    final protocol = _string(node['protocol']);
    final address = valid ? ip!.address : '';
    return ListTile(
      contentPadding: EdgeInsets.zero,
      title: Text(name.isNotEmpty ? name : _text(context, '未命名节点', 'Unnamed node')),
      subtitle: Text([
        if (valid) address else _text(context, '没有可用的 IP', 'No usable IP address'),
        if (protocol.isNotEmpty) protocol,
        if (node['latency'] != null) '${node['latency']} ms',
      ].join(' · ')),
      trailing: onFill == null
          ? null
          : TextButton(
              onPressed: valid
                  ? () => onFill!(ip?.type == InternetAddressType.IPv6 ? '[$address]' : address)
                  : null,
              child: Text(_text(context, '填入 IP', 'Fill IP')),
            ),
    );
  }
}

class _ProfileEditor extends StatefulWidget {
  const _ProfileEditor({required this.controller, this.profile});

  final EasyTierController controller;
  final _Entry? profile;

  @override
  State<_ProfileEditor> createState() => _ProfileEditorState();
}

class _ProfileEditorState extends State<_ProfileEditor> {
  final _form = GlobalKey<FormState>();
  late final TextEditingController _name, _network, _secret, _peers, _ipv4, _toml;
  final _fieldsChanged = <String>{};
  late bool _dhcp;
  bool _tomlTab = false;
  bool _tomlChanged = false;
  bool _saving = false;
  bool _secretVisible = false;
  String? _error;

  String t(String zh, String en) => _text(context, zh, en);

  @override
  void initState() {
    super.initState();
    final fields = widget.profile?['fields'];
    final existing = fields is _Entry ? fields : const <String, dynamic>{};
    final peers = existing['peers'];
    _name = TextEditingController(text: _string(widget.profile?['name']));
    _network = TextEditingController(text: _string(existing['network_name']));
    _secret = TextEditingController(text: _string(existing['secret']));
    _peers = TextEditingController(text: peers is List ? peers.whereType<String>().join('\n') : '');
    _ipv4 = TextEditingController(text: _string(existing['ipv4']));
    _toml = TextEditingController(text: _string(widget.profile?['toml']));
    _dhcp = existing['dhcp'] != false;
  }

  @override
  void dispose() {
    for (final controller in [_name, _network, _secret, _peers, _ipv4, _toml]) {
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_saving || _form.currentState?.validate() != true) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    final fields = <String, Object?>{
      if (_fieldsChanged.contains('network_name')) 'network_name': _network.text.trim(),
      if (_fieldsChanged.contains('secret')) 'secret': _secret.text,
      if (_fieldsChanged.contains('peers'))
        'peers': _peers.text.split(RegExp(r'[,\r\n]+'))
            .map((peer) => peer.trim()).where((peer) => peer.isNotEmpty).toList(),
      if (_fieldsChanged.contains('dhcp')) 'dhcp': _dhcp,
      if (_fieldsChanged.contains('ipv4')) 'ipv4': _ipv4.text.trim(),
    };
    final id = widget.profile?['id'];
    final ok = await widget.controller.command('save', {
      if (id is String) 'id': id,
      'name': _name.text.trim(),
      if (_tomlChanged) 'toml': _toml.text,
      if (fields.isNotEmpty) 'fields': fields,
    });
    if (!mounted) return;
    if (ok) {
      Navigator.pop(context);
    } else {
      setState(() {
        _saving = false;
        _error = widget.controller.lastCommandError.isNotEmpty
            ? widget.controller.lastCommandError
            : t('无法保存方案。', 'Could not save the profile.');
      });
    }
  }

  InputDecoration _decoration(String label, IconData icon, {Widget? suffix}) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final border = dark ? Colors.white12 : const Color(0xFFE1E6EE);
    return InputDecoration(
      labelText: label,
      labelStyle: TextStyle(color: dark ? Colors.white60 : const Color(0xFF737B8C), fontSize: 13),
      prefixIcon: Icon(icon, size: 19),
      suffixIcon: suffix,
      filled: true,
      fillColor: dark ? const Color(0xFF292C33) : const Color(0xFFF7F9FC),
      contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 15),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(10), borderSide: BorderSide(color: border)),
      disabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(10), borderSide: BorderSide(color: border)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(10), borderSide: BorderSide(color: Theme.of(context).colorScheme.primary, width: 1.4)),
      errorBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(10), borderSide: BorderSide(color: Theme.of(context).colorScheme.error)),
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(10)),
    );
  }

  Widget _field(
    String key,
    TextEditingController controller,
    String label, {
    bool secret = false,
    int lines = 1,
  }) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      controller: controller,
      enabled: !_saving,
      obscureText: secret && !_secretVisible,
      autocorrect: false,
      enableSuggestions: false,
      maxLines: lines,
      onChanged: (_) => _fieldsChanged.add(key),
      decoration: _decoration(
        label,
        secret ? Icons.lock_outline_rounded : key == 'network_name' ? Icons.hub_outlined : key == 'peers' ? Icons.route_outlined : Icons.lan_outlined,
        suffix: secret ? IconButton(
          tooltip: _secretVisible ? t('隐藏密码', 'Hide password') : t('显示密码', 'Show password'),
          onPressed: _saving ? null : () => setState(() => _secretVisible = !_secretVisible),
          icon: Icon(_secretVisible ? Icons.visibility_off_outlined : Icons.visibility_outlined),
        ) : null,
      ),
    ),
  );

  @override
  Widget build(BuildContext context) => AlertDialog(
    scrollable: true,
    backgroundColor: Theme.of(context).brightness == Brightness.dark ? const Color(0xFF202228) : Colors.white,
    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(18)),
    titlePadding: const EdgeInsets.fromLTRB(24, 22, 16, 18),
    contentPadding: const EdgeInsets.symmetric(horizontal: 24),
    actionsPadding: const EdgeInsets.fromLTRB(24, 12, 24, 20),
    title: Row(
      children: [
        Container(
          width: 42, height: 42,
          decoration: BoxDecoration(color: Theme.of(context).colorScheme.primary.withValues(alpha: .10), borderRadius: BorderRadius.circular(12)),
          child: Icon(Icons.hub_outlined, color: Theme.of(context).colorScheme.primary, size: 23),
        ),
        const SizedBox(width: 12),
        Expanded(child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('EasyTier · ${t('配置', 'Settings')}', style: const TextStyle(fontSize: 20, fontWeight: FontWeight.w700)),
            const SizedBox(height: 3),
            Text(widget.profile == null ? t('新建组网方案', 'New network profile') : t('编辑组网方案', 'Edit network profile'), style: TextStyle(fontSize: 12, color: Theme.of(context).hintColor)),
          ],
        )),
        IconButton(tooltip: t('关闭', 'Close'), onPressed: _saving ? null : () => Navigator.pop(context), icon: const Icon(Icons.close_rounded)),
      ],
    ),
    content: SizedBox(
      width: 620,
      child: Form(
        key: _form,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            TextFormField(
              controller: _name,
              enabled: !_saving,
              decoration: _decoration(t('方案名称', 'Profile name'), Icons.badge_outlined),
              validator: (v) => v?.trim().isNotEmpty == true
                  ? null
                  : t('请输入方案名称。', 'Enter a profile name.'),
            ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              children: [
                ChoiceChip(
                  label: Text(t('常用配置', 'Common settings')),
                  selected: !_tomlTab,
                  onSelected: _saving ? null : (_) => setState(() => _tomlTab = false),
                ),
                ChoiceChip(
                  label: const Text('TOML'),
                  selected: _tomlTab,
                  onSelected: _saving ? null : (_) => setState(() => _tomlTab = true),
                ),
              ],
            ),
            const SizedBox(height: 12),
            if (_tomlTab) ...[
              Text(t('表单中改动的字段将在保存时覆盖 TOML 中对应的值。', 'Changed fields in Common settings override the corresponding TOML values on save.')),
              const SizedBox(height: 8),
              TextField(
                controller: _toml,
                enabled: !_saving,
                onChanged: (_) => _tomlChanged = true,
                minLines: 10,
                maxLines: 13,
                autocorrect: false,
                enableSuggestions: false,
                style: const TextStyle(fontFamily: 'monospace'),
                decoration: _decoration('TOML', Icons.code_rounded),
              ),
            ] else ...[
              _field('network_name', _network, t('网络名称', 'Network name')),
              _field('secret', _secret, t('网络密钥', 'Network secret'), secret: true),
              _field('peers', _peers, t('引导节点（每行一个地址）', 'Bootstrap peers (one per line)'), lines: 2),
              Container(
                margin: const EdgeInsets.only(bottom: 12),
                decoration: BoxDecoration(
                  color: Theme.of(context).brightness == Brightness.dark ? const Color(0xFF292C33) : const Color(0xFFF7F9FC),
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: Theme.of(context).brightness == Brightness.dark ? Colors.white12 : const Color(0xFFE1E6EE)),
                ),
                child: SwitchListTile.adaptive(
                contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 4),
                secondary: const Icon(Icons.auto_awesome_outlined, size: 20),
                title: Text(t('自动分配虚拟 IP', 'Assign virtual IP automatically')),
                subtitle: const Text('DHCP'),
                value: _dhcp,
                onChanged: _saving ? null : (value) => setState(() {
                  _dhcp = value;
                  _fieldsChanged.add('dhcp');
                }),
              )),
              if (!_dhcp) _field('ipv4', _ipv4, t('固定虚拟 IPv4', 'Static virtual IPv4')),
            ],
            if (_error != null)
              Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            const Divider(height: 28),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(
        onPressed: _saving ? null : () => Navigator.pop(context),
        child: Text(t('取消', 'Cancel')),
      ),
      FilledButton.icon(
        onPressed: _saving ? null : _save,
        icon: _saving ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)) : const Icon(Icons.check_rounded, size: 18),
        label: Text(t('保存', 'Save')),
        style: FilledButton.styleFrom(padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 13), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(9))),
      ),
    ],
  );
}

class _ProfileNameDialog extends StatefulWidget {
  const _ProfileNameDialog({required this.title, required this.initial});

  final String title, initial;

  @override
  State<_ProfileNameDialog> createState() => _ProfileNameDialogState();
}

class _ProfileNameDialogState extends State<_ProfileNameDialog> {
  late final TextEditingController _controller = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final value = _controller.text.trim();
    if (value.isNotEmpty) Navigator.pop(context, value);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: Text(widget.title),
    content: TextField(
      controller: _controller,
      autofocus: true,
      onChanged: (_) => setState(() {}),
      onSubmitted: (_) => _submit(),
      decoration: InputDecoration(labelText: _text(context, '方案名称', 'Profile name')),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: Text(_text(context, '取消', 'Cancel')),
      ),
      FilledButton(
        onPressed: _controller.text.trim().isEmpty ? null : _submit,
        child: Text(_text(context, '确认', 'Confirm')),
      ),
    ],
  );
}
