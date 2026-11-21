import 'dart:async';
import 'dart:convert';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:uuid/uuid.dart';

import 'subnet_ui.dart';

/// 备份文件由本机后端加密处理，界面只提交当前操作的密码和路径。
class ConfigBackupPanel extends StatefulWidget {
  const ConfigBackupPanel({super.key});

  @override
  State<ConfigBackupPanel> createState() => _ConfigBackupPanelState();
}

class _ConfigBackupPanelState extends State<ConfigBackupPanel> {
  Timer? _timer;
  bool _ready = false, _preparing = false, _running = false;
  bool _restorePending = false;
  String? _requestId, _message, _error;
  DateTime? _submittedAt;
  String _action = '';
  bool _restoreResultSeen = false;

  bool get _busy => !_ready || _preparing || _running || _restorePending;
  String t(String chinese, String english) =>
      subnetText(context, chinese, english);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _readStatus();
    });
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _readStatus());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _readStatus() {
    if (!mounted) return;
    try {
      final status = jsonDecode(
        bind.mainGetLocalOption(key: 'config-backup-status'),
      );
      if (status is! Map<String, dynamic>) throw const FormatException();
      setState(() {
        _ready = true;
        final backendBusy = status['busy'] == true;
        _restorePending = status['restore_pending'] == true;
        if (_requestId != null &&
            status['request_id'] == _requestId &&
            !backendBusy) {
          if (status['success'] == true) {
            _message = switch (_action) {
              'restore' => _restoreMessage(),
              'cancel_restore' => t(
                '已取消待恢复，当前配置保持不变。',
                'Pending restore cancelled. The current configuration is unchanged.',
              ),
              _ => t('已导出加密备份。', 'Encrypted backup exported.'),
            };
            _error = null;
          } else {
            final error = status['error'];
            _error = error is String && error.isNotEmpty
                ? error
                : t('备份操作失败，请重试。', 'The backup action failed. Try again.');
          }
          _requestId = null;
        } else if (_requestId != null &&
            _submittedAt != null &&
            DateTime.now().difference(_submittedAt!) >
                const Duration(minutes: 2)) {
          _error = t(
            '等待操作结果超时，尚未确认完成。若后台仍在处理，请稍候。',
            'Timed out waiting for the result; completion has not been confirmed. The operation may still be running.',
          );
          _requestId = null;
        }
        _running = backendBusy || _requestId != null;
        if (_restorePending) _message = _restoreMessage();
        final restoreResult = status['restore_result'];
        if (!_restoreResultSeen &&
            restoreResult is Map<String, dynamic> &&
            !_restorePending &&
            !_preparing &&
            !_running) {
          _restoreResultSeen = true;
          // 启动恢复结果只呈现一次，不覆盖用户之后操作产生的反馈。
          if (_message == null && _error == null) {
            if (restoreResult['success'] == true) {
              final message = restoreResult['message'];
              _message = message is String && message.isNotEmpty
                  ? message
                  : t('配置已从备份恢复。', 'Configuration restored from backup.');
              final original = restoreResult['original_backup_path'];
              if (original is String && original.isNotEmpty) {
                _message =
                    '${_message!}\n${t('原配置副本：', 'Original configuration copy:')} $original';
              }
            } else {
              final error = restoreResult['error'];
              _error = error is String && error.isNotEmpty
                  ? error
                  : t(
                      '启动时未能恢复配置，请检查备份后重试。',
                      'Configuration could not be restored at startup. Check the backup and try again.',
                    );
            }
          }
        }
      });
    } catch (_) {
      setState(() {
        _ready = true;
        _error = t(
          '无法读取配置备份状态，请稍后重试。',
          'Could not read backup status. Try again shortly.',
        );
      });
    }
  }

  String _restoreMessage() => t(
    '已准备恢复配置。请完整退出 SubnetDesk 后重新打开，使恢复生效；原配置副本会保留。',
    'Restore is ready. Fully quit and reopen SubnetDesk to apply it. A copy of the original configuration will be retained.',
  );

  Future<void> _submit(
    String action, {
    String path = '',
    String password = '',
  }) async {
    final id = const Uuid().v4();
    setState(() {
      _requestId = id;
      _submittedAt = DateTime.now();
      _action = action;
      _running = true;
    });
    await bind.mainSetLocalOption(
      key: 'config-backup-command',
      value: jsonEncode({
        'action': action,
        'path': path,
        'password': password,
        'request_id': id,
      }),
    );
    if (mounted) _readStatus();
  }

  Future<void> _cancelRestore() async {
    if (!_ready || _preparing || _running || !_restorePending) return;
    setState(() {
      _preparing = true;
      _error = null;
    });
    try {
      await _submit('cancel_restore');
    } catch (_) {
      if (mounted) {
        setState(() {
          _requestId = null;
          _running = false;
          _error = t(
            '无法取消待恢复，请重试。',
            'Could not cancel the pending restore. Try again.',
          );
        });
      }
    } finally {
      if (mounted) setState(() => _preparing = false);
    }
  }

  Future<void> _perform({required bool restore}) async {
    if (_busy) return;
    setState(() {
      _preparing = true;
      _message = null;
      _error = null;
    });
    try {
      String? file;
      if (restore) {
        final result = await FilePicker.pickFiles(
          dialogTitle: t('选择配置备份', 'Choose a configuration backup'),
          type: FileType.custom,
          allowedExtensions: ['sdbak'],
          allowMultiple: false,
        );
        file = result?.files.single.path;
      } else {
        final stamp = DateTime.now()
            .toIso8601String()
            .split('.')
            .first
            .replaceAll(':', '-');
        file = await FilePicker.saveFile(
          dialogTitle: t('保存加密配置备份', 'Save an encrypted configuration backup'),
          fileName: 'SubnetDesk-$stamp.sdbak',
          type: FileType.custom,
          allowedExtensions: ['sdbak'],
        );
        if (file != null && !file.toLowerCase().endsWith('.sdbak')) {
          file = '$file.sdbak';
        }
      }
      if (!mounted || file == null) return;
      if (!file.toLowerCase().endsWith('.sdbak')) {
        setState(
          () => _error = t(
            '请选择 .sdbak 配置备份文件。',
            'Choose a .sdbak configuration backup file.',
          ),
        );
        return;
      }
      final password = await showDialog<String>(
        context: context,
        builder: (_) => _BackupPasswordDialog(restore: restore),
      );
      if (!mounted || password == null) return;
      await _submit(
        restore ? 'restore' : 'export',
        path: file,
        password: password,
      );
    } catch (_) {
      if (mounted) {
        setState(() {
          _requestId = null;
          _running = false;
          _error = t(
            '无法执行配置备份操作，请检查所选文件或保存位置后重试。',
            'Could not perform the backup action. Check the selected file or destination and try again.',
          );
        });
      }
    } finally {
      if (mounted) setState(() => _preparing = false);
    }
  }

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(
        t(
          '包含 EasyTier 方案、LAN 配置、收藏与分组等应用配置；不包含日志、录屏、缓存和系统自启动任务。',
          'Includes EasyTier profiles, LAN settings, favorites, groups and other app configuration; excludes logs, recordings, caches and system startup tasks.',
        ),
      ),
      const SizedBox(height: 8),
      Text(
        t(
          '备份包含敏感配置，请保管好文件和密码；仅支持在原电脑恢复。',
          'Backups contain sensitive configuration. Keep the file and password safe; restore is supported only on the original computer.',
        ),
        style: TextStyle(color: Theme.of(context).hintColor),
      ),
      const SizedBox(height: 16),
      Wrap(
        spacing: 12,
        runSpacing: 10,
        children: [
          FilledButton.icon(
            onPressed: _busy ? null : () => _perform(restore: false),
            icon: const Icon(Icons.file_upload_outlined),
            label: Text(t('导出加密备份', 'Export encrypted backup')),
          ),
          OutlinedButton.icon(
            onPressed: _busy ? null : () => _perform(restore: true),
            icon: const Icon(Icons.restore_rounded),
            label: Text(t('从备份恢复', 'Restore from backup')),
          ),
          if (_restorePending)
            TextButton.icon(
              onPressed: _preparing || _running ? null : _cancelRestore,
              icon: const Icon(Icons.cancel_outlined),
              label: Text(t('取消待恢复', 'Cancel pending restore')),
            ),
        ],
      ),
      if (_running) ...[
        const SizedBox(height: 14),
        const LinearProgressIndicator(),
        const SizedBox(height: 8),
        Text(
          t('正在处理配置备份或恢复，请稍候。', 'Processing a backup or restore. Please wait.'),
        ),
      ],
      if (_message != null || _error != null) ...[
        const SizedBox(height: 12),
        Text(
          _error ?? _message!,
          style: TextStyle(
            color: _error != null ? Theme.of(context).colorScheme.error : null,
          ),
        ),
      ],
    ],
  );
}

class _BackupPasswordDialog extends StatefulWidget {
  const _BackupPasswordDialog({required this.restore});
  final bool restore;

  @override
  State<_BackupPasswordDialog> createState() => _BackupPasswordDialogState();
}

class _BackupPasswordDialogState extends State<_BackupPasswordDialog> {
  final _form = GlobalKey<FormState>();
  final _password = TextEditingController(),
      _confirmation = TextEditingController();
  bool _showPassword = false, _showConfirmation = false;
  String t(String chinese, String english) =>
      subnetText(context, chinese, english);

  @override
  void dispose() {
    _password.clear();
    _confirmation.clear();
    _password.dispose();
    _confirmation.dispose();
    super.dispose();
  }

  Widget _field({bool confirmation = false}) {
    final visible = confirmation ? _showConfirmation : _showPassword;
    return Padding(
      padding: const EdgeInsets.only(top: 14),
      child: TextFormField(
        controller: confirmation ? _confirmation : _password,
        obscureText: !visible,
        autocorrect: false,
        enableSuggestions: false,
        autofocus: !confirmation,
        decoration: InputDecoration(
          labelText: confirmation
              ? t('确认密码', 'Confirm password')
              : t('备份密码', 'Backup password'),
          prefixIcon: const Icon(Icons.lock_outline_rounded),
          filled: true,
          fillColor: Theme.of(context).brightness == Brightness.dark
              ? const Color(0xFF292C34)
              : const Color(0xFFF7F9FC),
          border: OutlineInputBorder(borderRadius: BorderRadius.circular(10)),
          suffixIcon: IconButton(
            tooltip: visible
                ? t('隐藏密码', 'Hide password')
                : t('显示密码', 'Show password'),
            onPressed: () => setState(() {
              if (confirmation) {
                _showConfirmation = !_showConfirmation;
              } else {
                _showPassword = !_showPassword;
              }
            }),
            icon: Icon(
              visible
                  ? Icons.visibility_off_outlined
                  : Icons.visibility_outlined,
            ),
          ),
        ),
        validator: (value) {
          if (confirmation) {
            return value == _password.text
                ? null
                : t('两次密码不一致。', 'Passwords do not match.');
          }
          final length = utf8.encode(value ?? '').length;
          if (length < 8) {
            return t(
              '密码过短，请使用更长的密码（建议至少 8 个字符）。',
              'The password is too short. Use a longer password (at least 8 characters recommended).',
            );
          }
          if (length > 1024) {
            return t(
              '密码过长，请缩短后重试。',
              'The password is too long. Shorten it and try again.',
            );
          }
          return null;
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    scrollable: true,
    backgroundColor: Theme.of(context).brightness == Brightness.dark
        ? const Color(0xFF202228)
        : Colors.white,
    shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(18)),
    title: Text(
      widget.restore
          ? t('覆盖并恢复配置', 'Overwrite and restore configuration')
          : t('导出加密备份', 'Export encrypted backup'),
    ),
    content: SizedBox(
      width: 440,
      child: Form(
        key: _form,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              widget.restore
                  ? t(
                      '此操作将用备份覆盖当前应用配置，仅支持在原电脑恢复。完整退出后，下次启动应用时生效；恢复前会保留原配置副本。',
                      'This will overwrite the current app configuration. Restore is supported only on the original computer and applies the next time the app starts after fully quitting. A copy of the original configuration will be retained.',
                    )
                  : t(
                      '请设置备份密码并妥善保管；恢复时需要此密码。',
                      'Set a backup password and keep it safe. It is required to restore this file.',
                    ),
            ),
            _field(),
            if (!widget.restore) _field(confirmation: true),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.of(context).pop(),
        child: Text(t('取消', 'Cancel')),
      ),
      FilledButton.icon(
        onPressed: () {
          if (_form.currentState?.validate() == true) {
            Navigator.of(context).pop(_password.text);
          }
        },
        icon: Icon(
          widget.restore ? Icons.restore_rounded : Icons.lock_outline_rounded,
        ),
        label: Text(
          widget.restore
              ? t('确认覆盖并准备恢复', 'Confirm overwrite and prepare restore')
              : t('加密并导出', 'Encrypt and export'),
        ),
      ),
    ],
  );
}
