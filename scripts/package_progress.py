"""Human-readable build progress; raw tool output remains in run.log."""
from collections import deque
import re
import time


class Progress:
    def __init__(self, mode, verbose=False, emit=print, clock=time.monotonic):
        self.mode, self.verbose, self.emit, self.clock = mode, verbose, emit, clock
        self.started = self.changed = self.last_update = clock()
        self.stage = None
        self.pending = ''
        self.tail = deque(maxlen=18)
        self.warnings = set()
        self.set_stage('检查环境并启动构建工具')

    def set_stage(self, name):
        if self.stage == name:
            return
        now = self.clock()
        if self.stage:
            self.emit(f'  完成 {self.stage} · {now - self.changed:.1f} 秒')
        self.stage, self.changed, self.last_update = name, now, now
        self.emit(f'▶ {name}')

    def feed(self, text):
        self.pending += text
        while '\n' in self.pending:
            line, self.pending = self.pending.split('\n', 1)
            self.line(line.rstrip('\r'))

    def line(self, raw):
        line = re.sub(r'\x1b\[[0-9;]*[a-zA-Z]', '', raw).strip()
        if not line:
            return
        self.tail.append(raw)
        if self.verbose:
            self.emit(raw)
        if line.startswith('UIPILOT_PROVIDER_BUILD'):
            self.set_stage('构建截图 Provider')
        elif line.startswith('UIPILOT_PROVIDER_READY'):
            self.set_stage('校验截图 Provider 与构建前端')
        elif line.startswith('UIPILOT_PROVIDER_VALIDATED'):
            self.set_stage('校验 Provider 并准备安装资源')
        elif re.match(r'> \S+@\S+ build$', line):
            self.set_stage('构建前端（类型检查与资源生成）')
        elif re.match(r'> \S+@\S+ build:managed-everything$', line):
            self.set_stage('构建 Everything 安装助手')
        elif line.startswith('MANAGED_EVERYTHING_HELPER_BUILD_PASS'):
            self.set_stage('编译主程序（Rust Release）')
        elif 'Built application at:' in line:
            self.set_stage('生成 NSIS 安装包')
        elif line.startswith('SHA256 '):
            self.set_stage('校验产物并发布')
        elif line.startswith('UIPILOT_UNCHANGED_ASSETS '):
            self.emit(f'  {line.split()[-1]} 个前端文件内容未变，保留编译缓存')
        else:
            for marker in ('构建插件 CLI', '校验插件源码', '生成插件包', '校验插件归档'):
                if re.match(r'\[\d\d:\d\d:\d\d\] ' + marker, line):
                    self.set_stage(marker)
                    break
        warning = line.startswith(('warning:', '(!)')) and not re.match(r'warning: .* generated \d+ warnings?', line)
        if warning and line not in self.warnings:
            self.warnings.add(line)
            if not self.verbose:
                message = line.removeprefix('warning:').removeprefix('(!)').strip()
                if 'Some chunks are larger than' in line:
                    message = '部分前端资源超过建议体积；不阻断打包，详情见日志'
                self.emit(f'  警告：{message}')

    def tick(self):
        now = self.clock()
        if now - self.last_update >= 15:
            self.emit(f'  {self.stage} · 已用 {now - self.changed:.0f} 秒（总计 {now - self.started:.0f} 秒）')
            self.last_update = now

    def finish(self, success):
        if self.pending:
            self.line(self.pending)
            self.pending = ''
        now = self.clock()
        if success:
            self.emit(f'  完成 {self.stage} · {now - self.changed:.1f} 秒')
            self.emit(f'总耗时：{now - self.started:.1f} 秒 · {len(self.warnings)} 条不同警告（详情见日志）')
        elif self.tail and not self.verbose:
            self.emit('最后的构建输出：\n' + '\n'.join(self.tail))
