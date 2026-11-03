"""Machine-local entry point: remember paths, then delegate dev to the chosen repo."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import tempfile


def read_config(path):
    if not path.is_file():
        return {}
    value = json.loads(path.read_text(encoding='utf-8-sig'))
    if not isinstance(value, dict):
        raise ValueError(f'配置格式无效：{path}')
    return value


def ask_yes_no(question, default):
    while True:
        value = input(question).strip().lower()
        if not value:
            return default
        if value in ('y', 'yes', 'n', 'no'):
            return value in ('y', 'yes')
        print('请输入 Y 或 N。')


def prompt_path(label, default):
    value = input(f'{label} [{default}]：').strip().strip('"')
    return Path(value or default).expanduser().resolve()


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--root', type=Path, default=None)
    parser.add_argument('--rust-uia', type=Path, default=None)
    parser.add_argument('--non-interactive', action='store_true')
    args, remaining = parser.parse_known_args()
    config = Path(__file__).resolve().with_name('dev-uipilot.config.json')
    try:
        saved = read_config(config)
        root = (args.root or Path(saved.get('root', 'D:/code/UiPilot_tools'))).resolve()
        source = args.rust_uia or saved.get('rust_uia')
        if source is None:
            source = read_config(root / '.uipilot-local.json').get('rust_uia', 'D:/code/rust_uia')
        source = Path(source).resolve()
        help_only = '--help' in remaining or '-h' in remaining
        if not help_only and not args.non_interactive and sys.stdin.isatty() and args.root is None and args.rust_uia is None:
            if saved:
                print(f'上次项目：{root}\n上次 rust_uia：{source}', flush=True)
            reuse = bool(saved) and ask_yes_no('是否使用上一次的配置？(Y/n) ', True)
            if not reuse:
                root = prompt_path('UiPilot 项目路径 (--root)', str(root))
                source = prompt_path('rust_uia 路径 (--rust-uia)', str(source))
        script = root / 'scripts/dev_uipilot.py'
        if not script.is_file():
            raise ValueError(f'UiPilot dev script does not exist: {script}; use --root')
        if not help_only:
            saved.update(root=str(root), rust_uia=str(source))
            with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=config.parent, delete=False) as file:
                temporary = Path(file.name)
                json.dump(saved, file, ensure_ascii=False, indent=2)
            try:
                os.replace(temporary, config)
            finally:
                temporary.unlink(missing_ok=True)
        options = ['--non-interactive'] if args.non_interactive else []
        return subprocess.call([sys.executable, str(script), '--root', str(root), '--rust-uia', str(source), *options, *remaining])
    except (ValueError, OSError, EOFError) as error:
        print(f'[UiPilot dev] 启动失败：{error}', file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == '__main__':
    raise SystemExit(main())
