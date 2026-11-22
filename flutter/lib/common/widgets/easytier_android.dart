import 'dart:convert';

import 'package:flutter/services.dart';

/// Android 的服务通道；TOML 文档选择由原生 SAF 处理，不能把 URI 当文件路径。
class EasyTierAndroidBridge {
  static const _channel = MethodChannel('com.zibochen.subnetdesk/easytier');

  Future<String> status() async =>
      await _channel.invokeMethod<String>('status') ?? '{}';

  Future<String> command(Map<String, Object?> request) async =>
      await _channel.invokeMethod<String>('command', {'request': jsonEncode(request)}) ?? '{}';

  Future<String> readProfile(String id) async =>
      await _channel.invokeMethod<String>('readProfile', {'id': id}) ?? 'null';

  Future<String> importProfile(String requestId, String name) async =>
      await _channel.invokeMethod<String>('importProfile', {
        'request_id': requestId, 'name': name,
      }) ?? '{}';

  Future<String> exportProfile(String requestId, String id) async =>
      await _channel.invokeMethod<String>('exportProfile', {
        'request_id': requestId, 'id': id,
      }) ?? '{}';
}
