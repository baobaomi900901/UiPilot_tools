"""Machine-local thin entry point; all workflow logic lives in UiPilot."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import tempfile

def main():
    parser=argparse.ArgumentParser(add_help=False)
    parser.add_argument('--root',type=Path,default=None)
    args,remaining=parser.parse_known_args()
    config=Path(__file__).resolve().with_name('dev-uipilot.config.json')
    saved=json.loads(config.read_text(encoding='utf-8')) if config.is_file() else {}
    root=(args.root or Path(saved.get('root','D:/code/UiPilot_tools'))).resolve()
    script=root/'scripts/dev_uipilot.py'
    if not script.is_file():
        parser.error(f'UiPilot dev script does not exist: {script}; use --root')
    if args.root is not None:
        saved['root']=str(root)
        with tempfile.NamedTemporaryFile(mode='w',encoding='utf-8',dir=config.parent,delete=False) as file:
            temporary=Path(file.name)
            json.dump(saved,file,ensure_ascii=False,indent=2)
        try: os.replace(temporary,config)
        finally: temporary.unlink(missing_ok=True)
    return subprocess.call([sys.executable,str(script),'--root',str(root),*remaining])

if __name__=='__main__': raise SystemExit(main())
