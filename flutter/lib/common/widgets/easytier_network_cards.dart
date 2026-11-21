import 'dart:async';

import 'package:flutter/material.dart';

import 'subnet_ui.dart';

typedef _Node = Map<String, dynamic>;

String _str(Object? value) => value is String ? value : '';
double? _number(Object? value) => value is num && value.isFinite && value > 0
    ? value.toDouble() : null;
String _ms(Object? value, {bool estimated = false}) {
  final number = _number(value);
  if (number == null) return '—';
  return '${estimated ? '≈ ' : ''}${number.toStringAsFixed(1)} ms';
}
String _kind(_Node node, bool stale) => stale ? 'unknown' : switch (node['path_kind']) {
  'direct' => 'direct', 'relay' => 'relay', _ => 'unknown',
};
String _name(_Node node, BuildContext context) => _str(node['name']).isEmpty
    ? subnetText(context, '未命名设备', 'Unnamed device') : _str(node['name']);
String _hop(_Node node) => _str(node['next_hop_name']).isNotEmpty
    ? _str(node['next_hop_name']) : _str(node['next_hop_ip']);

/// 纯展示组件：桌面和手机共用数据，不启动组网或依赖桌面窗口接口。
class EasyTierNetworkCards extends StatefulWidget {
  const EasyTierNetworkCards({
    super.key, required this.nodes, required this.running,
    this.stale = false, this.queriedAt, this.onFill, this.onRefresh,
    this.emptyMessage,
  });
  final List<Map<String, dynamic>> nodes;
  final bool running, stale;
  final DateTime? queriedAt;
  final ValueChanged<String>? onFill;
  final VoidCallback? onRefresh;
  final String? emptyMessage;

  @override
  State<EasyTierNetworkCards> createState() => _EasyTierNetworkCardsState();
}

class _EasyTierNetworkCardsState extends State<EasyTierNetworkCards> {
  String _search = '', _filter = 'all';
  ValueNotifier<_DetailData>? _detail;
  String t(String zh, String en) => subnetText(context, zh, en);

  @override
  void didUpdateWidget(covariant EasyTierNetworkCards oldWidget) {
    super.didUpdateWidget(oldWidget);
    final detail = _detail;
    if (detail == null) return;
    final selected = detail.value.node;
    final id = _str(selected['id']);
    final current = widget.running ? widget.nodes.where((node) => id.isNotEmpty
      ? _str(node['id']) == id : node['address'] == selected['address']).firstOrNull : null;
    final next = _DetailData(current ?? selected, widget.queriedAt, widget.stale || current == null,
      unavailable: current == null);
    // 弹层位于另一条 Overlay 分支，父列表构建时不能同步触发其 setState。
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (identical(_detail, detail)) detail.value = next;
    });
  }

  void _details(_Node node) {
    if (_detail != null) return;
    final detail = ValueNotifier(_DetailData(node, widget.queriedAt, widget.stale));
    _detail = detail;
    final content = ValueListenableBuilder<_DetailData>(valueListenable: detail,
      builder: (_, value, __) => _NetworkDetails(node: value.node, queriedAt: value.queriedAt,
        stale: value.stale, unavailable: value.unavailable));
    late final Future<dynamic> closed;
    if (MediaQuery.sizeOf(context).width < 700) {
      closed = showModalBottomSheet<void>(
        context: context, isScrollControlled: true, useSafeArea: true,
        showDragHandle: true,
        builder: (_) => DraggableScrollableSheet(
          expand: false, initialChildSize: .78, minChildSize: .4, maxChildSize: .95,
          builder: (_, scroll) => SingleChildScrollView(controller: scroll,
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 28), child: content),
        ),
      );
    } else {
      closed = showDialog<void>(context: context, builder: (dialog) => AlertDialog(
        scrollable: true, title: Text(t('设备与路由详情', 'Device and route details')),
        content: SizedBox(width: 540, child: content),
        actions: [TextButton(onPressed: () => Navigator.pop(dialog), child: Text(t('关闭', 'Close')))],
      ));
    }
    closed.whenComplete(() {
      if (identical(_detail, detail)) _detail = null;
      detail.dispose();
    });
  }

  @override
  Widget build(BuildContext context) {
    final nodes = widget.running ? widget.nodes : <_Node>[];
    final filtered = nodes.where((node) {
      final matches = '${_str(node['name'])} ${_str(node['address'])} ${_hop(node)}'
          .toLowerCase().contains(_search.toLowerCase());
      return matches && (_filter == 'all' || _kind(node, widget.stale) == _filter);
    }).toList();
    final direct = nodes.where((node) => _kind(node, widget.stale) == 'direct').length;
    final relay = nodes.where((node) => _kind(node, widget.stale) == 'relay').length;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Row(children: [
        Expanded(child: Text(t('组网设备', 'Network devices'), style: Theme.of(context).textTheme.titleLarge)),
        IconButton(tooltip: t('刷新组网设备', 'Refresh network devices'), onPressed: widget.onRefresh,
          icon: const Icon(Icons.refresh_rounded), constraints: const BoxConstraints(minWidth: 48, minHeight: 48)),
      ]),
      Wrap(spacing: 14, runSpacing: 6, children: [
        Text(t('设备 ${nodes.length}', '${nodes.length} devices')),
        Text(t('直连 $direct', '$direct direct')),
        Text(t('中继 $relay', '$relay relayed')),
        Text(t('未知 ${nodes.length - direct - relay}', '${nodes.length - direct - relay} unknown')),
      ]),
      if (widget.queriedAt != null) Padding(padding: const EdgeInsets.only(top: 6),
        child: Text(t('查询于 ${_time(context, widget.queriedAt!)}', 'Queried at ${_time(context, widget.queriedAt!)}'),
          style: Theme.of(context).textTheme.bodySmall)),
      if (widget.stale && widget.running) Padding(padding: const EdgeInsets.only(top: 10),
        child: Text(t('查询结果已过期或暂时不可用，路径状态待刷新。', 'Query results are stale or unavailable. Refresh to confirm paths.'),
          style: TextStyle(color: Theme.of(context).colorScheme.error))),
      const SizedBox(height: 14),
      TextField(
        key: const ValueKey('easytier-network-search'),
        decoration: InputDecoration(prefixIcon: const Icon(Icons.search_rounded),
          hintText: t('搜索设备名、IP 或中继节点', 'Search device, IP or relay')),
        onChanged: (value) => setState(() => _search = value.trim()),
      ),
      const SizedBox(height: 10),
      Wrap(spacing: 8, runSpacing: 8, children: [
        for (final value in ['all', 'direct', 'relay', 'unknown']) ChoiceChip(
          key: ValueKey('easytier-filter-$value'),
          selected: _filter == value, onSelected: (_) => setState(() => _filter = value),
          label: Text(switch (value) {
            'direct' => t('直连', 'Direct'), 'relay' => t('中继', 'Relayed'),
            'unknown' => t('未知', 'Unknown'), _ => t('全部', 'All'),
          }),
        ),
      ]),
      const SizedBox(height: 14),
      if (filtered.isEmpty) Padding(padding: const EdgeInsets.symmetric(vertical: 24),
        child: Text(nodes.isEmpty ? widget.emptyMessage ?? t('当前没有设备。', 'No devices in this network.')
          : t('没有匹配的设备。', 'No matching devices.')))
      else LayoutBuilder(builder: (context, constraints) {
        final columns = constraints.maxWidth >= 860 && MediaQuery.textScalerOf(context).scale(16) < 24 ? 2 : 1;
        final width = (constraints.maxWidth - (columns - 1) * 14) / columns;
        return Wrap(spacing: 14, runSpacing: 14, children: [
          for (final node in filtered) SizedBox(width: width,
            child: _card(node)),
        ]);
      }),
    ]);
  }

  Widget _card(_Node node) {
    final colors = Theme.of(context).colorScheme;
    final kind = _kind(node, widget.stale);
    final ip = _str(node['address']);
    final hop = _hop(node);
    return Material(
      key: ValueKey('easytier-card-${_str(node['id']).isEmpty ? ip : node['id']}'),
      color: colors.surface,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(18), side: BorderSide(color: colors.outline)),
      clipBehavior: Clip.antiAlias,
      child: InkWell(onTap: () => _details(node), child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            CircleAvatar(backgroundColor: colors.surfaceContainerHighest, foregroundColor: colors.onSurface,
              child: const Icon(Icons.computer_rounded, size: 22)),
            const SizedBox(width: 12),
            Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(_name(node, context), maxLines: 2, overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w600)),
              const SizedBox(height: 4), Text(ip, softWrap: true),
            ])),
            IconButton(
              key: ValueKey('easytier-fill-$ip'), tooltip: t('填入远控 IP', 'Fill remote IP'),
              constraints: const BoxConstraints(minWidth: 48, minHeight: 48),
              onPressed: widget.onFill == null ? null : () => widget.onFill!(ip),
              icon: const Icon(Icons.link_rounded),
            ),
          ]),
          const SizedBox(height: 12),
          Wrap(spacing: 12, runSpacing: 8, crossAxisAlignment: WrapCrossAlignment.center, children: [
            _PathBadge(kind: kind, hops: node['hops']),
            Text(_ms(kind == 'direct' ? _number(node['path_latency']) ?? node['direct_latency'] : kind == 'relay' ? node['path_latency'] : null,
              estimated: kind == 'relay'), style: Theme.of(context).textTheme.titleMedium),
            if (kind == 'direct' && _str(node['protocol']).isNotEmpty)
              Text(_str(node['protocol']).toUpperCase(), style: Theme.of(context).textTheme.bodySmall),
          ]),
          const SizedBox(height: 10),
          LayoutBuilder(builder: (context, constraints) {
            final hopIp = _str(node['next_hop_ip']);
            final via = constraints.maxWidth >= 420 && hopIp.isNotEmpty && hopIp != hop ? '$hop · $hopIp' : hop;
            return Row(children: [
              Expanded(child: Text(kind == 'relay' ? (hop.isEmpty ? t('中继节点未知', 'Relay unknown') : t('经 $via', 'Via $via'))
                : kind == 'direct' ? t('本机 → 目标设备', 'This device → destination') : t('等待确认路径', 'Waiting for path information'),
                maxLines: 2, overflow: TextOverflow.ellipsis)),
              IconButton(onPressed: () => _details(node), tooltip: t('详情', 'Details'),
                constraints: const BoxConstraints(minWidth: 48, minHeight: 48), icon: const Icon(Icons.chevron_right_rounded)),
            ]);
          }),
        ]),
      )),
    );
  }
}

String _time(BuildContext context, DateTime time) => MaterialLocalizations.of(context)
    .formatTimeOfDay(TimeOfDay.fromDateTime(time));

class _DetailData {
  const _DetailData(this.node, this.queriedAt, this.stale, {this.unavailable = false});
  final _Node node;
  final DateTime? queriedAt;
  final bool stale, unavailable;
}

class _PathBadge extends StatelessWidget {
  const _PathBadge({required this.kind, this.hops});
  final String kind;
  final Object? hops;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final label = switch (kind) {
      'direct' => subnetText(context, '直连', 'Direct'),
      'relay' => subnetText(context, '中继${hops is num ? ' · $hops 跳' : ''}', 'Relayed${hops is num ? ' · $hops hops' : ''}'),
      _ => subnetText(context, '路径未知', 'Path unknown'),
    };
    return DecoratedBox(decoration: BoxDecoration(
      color: colors.surfaceContainerHighest, borderRadius: BorderRadius.circular(20)),
      child: Padding(padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(switch (kind) {'direct' => Icons.swap_horiz_rounded, 'relay' => Icons.alt_route_rounded, _ => Icons.help_outline_rounded}, size: 18),
          const SizedBox(width: 6), Flexible(child: Text(label)),
        ])),
    );
  }
}

class _NetworkDetails extends StatefulWidget {
  const _NetworkDetails({required this.node, required this.queriedAt, required this.stale, this.unavailable = false});
  final _Node node;
  final DateTime? queriedAt;
  final bool stale;
  final bool unavailable;
  @override
  State<_NetworkDetails> createState() => _NetworkDetailsState();
}

class _NetworkDetailsState extends State<_NetworkDetails> {
  Timer? _timer;
  @override
  void initState() {
    super.initState();
    // 详情展示打开时的快照，超过有效期后不能继续声称是当前路径。
    _timer = Timer.periodic(const Duration(seconds: 1), (_) { if (mounted) setState(() {}); });
  }
  @override
  void dispose() { _timer?.cancel(); super.dispose(); }
  String t(String zh, String en) => subnetText(context, zh, en);
  @override
  Widget build(BuildContext context) {
    final node = widget.node;
    final stale = widget.stale || widget.queriedAt == null || DateTime.now().difference(widget.queriedAt!) > const Duration(seconds: 15);
    final kind = _kind(node, stale);
    final hop = _hop(node);
    Widget step(String title, [String subtitle = '']) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        const Icon(Icons.radio_button_checked_rounded, size: 18), const SizedBox(width: 10),
        Expanded(child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(title), if (subtitle.isNotEmpty) SelectableText(subtitle),
        ])),
      ]),
    );
    Widget metric(String title, String value) => Padding(padding: const EdgeInsets.symmetric(vertical: 5),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(title, style: Theme.of(context).textTheme.bodySmall),
        const SizedBox(height: 2), SelectableText(value),
      ]));
    final subnets = node['proxy_cidrs'] is List ? (node['proxy_cidrs'] as List).whereType<String>().toList() : <String>[];
    return Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
      Text(_name(node, context), style: Theme.of(context).textTheme.titleLarge),
      const SizedBox(height: 6), SelectableText(_str(node['address'])),
      const SizedBox(height: 12), _PathBadge(kind: kind, hops: node['hops']),
      if (stale) Padding(padding: const EdgeInsets.only(top: 10),
        child: Text(widget.unavailable ? t('设备已不在当前列表中，路径暂不可用。', 'This device is no longer in the current list; its path is unavailable.')
          : t('此快照已过期，正在等待新的查询结果。', 'This snapshot is stale. Waiting for new query results.'), style: TextStyle(color: Theme.of(context).colorScheme.error))),
      if (widget.queriedAt != null) metric(t('数据快照', 'Snapshot'), _time(context, widget.queriedAt!)),
      const Divider(height: 28),
      Text(t('选路', 'Route'), style: Theme.of(context).textTheme.titleMedium),
      metric(t('当前策略', 'Policy'), switch (node['path_strategy']) {
        'latency_first' => t('延迟优先', 'Latency first'),
        'least_hops_fallback' => t('延迟路由无效，回退最少跳数', 'Latency route unavailable; least hops fallback'),
        'least_hops' => t('最少跳数', 'Least hops'), _ => '—',
      }),
      step(t('本机', 'This device')),
      if (kind == 'relay') ...[
        const Padding(padding: EdgeInsets.only(left: 2), child: Icon(Icons.south_rounded, size: 18)),
        step(hop.isEmpty ? t('未知中继', 'Unknown relay') : hop, _str(node['next_hop_ip'])),
        if (node['hops'] is num && (node['hops'] as num) > 2) step('…', t('其他中间节点未提供', 'Further intermediate nodes not reported')),
      ],
      if (kind != 'unknown') ...[
        const Padding(padding: EdgeInsets.only(left: 2), child: Icon(Icons.south_rounded, size: 18)),
        step(_name(node, context), _str(node['address'])),
      ],
      metric(kind == 'relay' ? t('路径估计延迟', 'Estimated path latency') : t('直连延迟', 'Direct link latency'),
        _ms(kind == 'direct' ? _number(node['path_latency']) ?? node['direct_latency'] : kind == 'relay' ? node['path_latency'] : null, estimated: kind == 'relay')),
      if (kind == 'relay') metric(t('本机到下一跳', 'This device to next hop'), _ms(node['next_hop_latency'])),
      if (node['direct_available'] == true && kind == 'relay') ...[
        const SizedBox(height: 8), Text(t('直连链路可用，当前策略选择中继。', 'A direct link is available; the current policy selects a relay.')),
        metric(t('备用直连延迟', 'Available direct link latency'), _ms(node['direct_latency'])),
      ],
      if ((kind == 'direct' || node['direct_available'] == true) && _str(node['protocol']).isNotEmpty)
        metric(t('直连隧道协议', 'Direct tunnel protocol'), _str(node['protocol']).toUpperCase()),
      const Divider(height: 28),
      for (final field in {'loss': t('直连丢包', 'Direct link loss'), 'rx_bytes': t('直连累计接收', 'Direct link received total'),
        'tx_bytes': t('直连累计发送', 'Direct link sent total'), 'nat_type': t('NAT 类型', 'NAT type'), 'version': t('版本', 'Version')}.entries)
        if (_str(node[field.key]).isNotEmpty) metric(field.value, _str(node[field.key])),
      if (subnets.isNotEmpty) ...[
        const Divider(height: 28), Text(t('发布的代理子网', 'Advertised subnets'), style: Theme.of(context).textTheme.titleMedium),
        for (final subnet in subnets) metric(t('子网', 'Subnet'), subnet),
      ],
    ]);
  }
}
