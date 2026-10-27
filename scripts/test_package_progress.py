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


if __name__ == '__main__':
    unittest.main()
