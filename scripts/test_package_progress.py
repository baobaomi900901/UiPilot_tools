import unittest
from package_progress import Progress


class ProgressTests(unittest.TestCase):
    def test_chunked_output_stages_warnings_and_heartbeat(self):
        messages = []
        now = [0.0]
        progress = Progress('setup', emit=messages.append, clock=lambda: now[0])
        progress.feed('> uipilot@0.14.0 bui')
        progress.feed('ld\r\nnoise\nwarning: unused method\n  --> source:3\n')
        progress.feed('warning: unused method\n')
        now[0] = 20
        progress.tick()
        progress.feed('> uipilot@0.14.0 build:managed-everything\n')
        progress.feed('MANAGED_EVERYTHING_HELPER_BUILD_PASS path\n')
        text = '\n'.join(messages)
        self.assertIn('前端', text)
        self.assertIn('20', text)
        self.assertIn('主程序', text)
        self.assertEqual(text.count('unused method'), 1)
        self.assertNotIn('source:3', text)
        self.assertNotIn('noise', text)

    def test_failure_retains_last_partial_line_and_verbose_preserves_raw_text(self):
        messages = []
        progress = Progress('plugin', emit=messages.append)
        progress.feed('detail\nfatal error without newline')
        progress.finish(False)
        self.assertIn('fatal error without newline', '\n'.join(messages))
        messages.clear()
        progress = Progress('plugin', verbose=True, emit=messages.append)
        progress.feed('raw output\n')
        self.assertIn('raw output', '\n'.join(messages))

    def test_plugin_stages_and_compiler_warning_summary(self):
        messages = []
        progress = Progress('plugin', emit=messages.append)
        for stage in ('构建插件 CLI', '校验插件源码', '生成插件包', '校验插件归档'):
            progress.feed(f'[12:34:56] {stage}\n')
            self.assertEqual(progress.stage, stage)
        progress.feed('warning: unused code\nwarning: `app` (lib) generated 1 warning\n')
        progress.finish(True)
        self.assertEqual(len(progress.warnings), 1)
        self.assertIn('总耗时', '\n'.join(messages))


    def test_long_rust_build_reports_last_target_and_output_silence(self):
        messages = []
        now = [0.0]
        progress = Progress('setup', emit=messages.append, clock=lambda: now[0])
        progress.feed('> uipilot@0.16.0 build:managed-everything\n')
        now[0] = 3.0
        progress.feed('   Compiling windows v0.61.3\n')
        now[0] = 20.0
        progress.tick()
        text = '\n'.join(messages)
        self.assertIn('Compiling windows v0.61.3', text)
        self.assertIn('17 秒无新输出', text)
        progress.feed('MANAGED_EVERYTHING_HELPER_BUILD_PASS path\n')
        now[0] = 40.0
        progress.tick()
        self.assertNotIn('Compiling windows', messages[-1])

    def test_plugin_cli_build_is_not_reported_as_frontend_build(self):
        messages = []
        progress = Progress('plugin', emit=messages.append)
        progress.feed('[15:21:43] 构建插件 CLI\n> @uipilot/plugin-cli@0.1.0 build\n')
        self.assertEqual(progress.stage, '构建插件 CLI')
        self.assertNotIn('构建前端', '\n'.join(messages))

    def test_cargo_lock_wait_is_visible_without_verbose_output(self):
        messages = []
        progress = Progress('setup', emit=messages.append)
        progress.feed('    Blocking waiting for file lock on build directory\n')
        self.assertIn('等待 Cargo 构建目录锁', '\n'.join(messages))


if __name__ == '__main__':
    unittest.main()
