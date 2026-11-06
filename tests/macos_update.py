#!/usr/bin/env python3
"""Exercise bundle replacement and rollback with disposable apps and injected OS tools."""
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'packaging/macos/install-update.sh'


class MacOSUpdateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='ctdo-update-test-')
        self.root = Path(self.temporary.name)
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        self.app = self.root / 'Applications with spaces/ctdozlite.app'
        self.new = self.root / 'new/ctdozlite.app'
        for app, version in [(self.app, '0.0.8'), (self.new, '0.0.9')]:
            (app / 'Contents/MacOS').mkdir(parents=True)
            (app / 'Contents/MacOS/ctdozlite').write_text(version)
            (app / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleShortVersionString': version}))
        self.download = self.root / 'private download'
        self.download.mkdir()
        self.dmg = self.download / 'update.dmg'
        self.dmg.write_bytes(b'validated-download-fixture')
        self.env = dict(os.environ, PATH=f'{self.tools}:' + os.environ['PATH'], FIXTURE_BUNDLE=str(self.new))
        commands = {
            'hdiutil': '''if [[ "$1" == attach ]]; then
  [[ "${FAIL_MOUNT:-0}" == 0 ]] || exit 1
  shift
  while [[ "$1" != -mountpoint ]]; do shift; done
  cp -R "$FIXTURE_BUNDLE" "$2/ctdozlite.app"
fi''',
            'codesign': '[[ "${FAIL_SIGNATURE:-0}" == 0 ]]',
            'lipo': 'echo "${FIXTURE_ARCH:-arm64}"',
            'ditto': '[[ "${FAIL_COPY:-0}" == 0 ]] && cp -R "$1" "$2"',
            'sleep': 'exit 0',
            'mv': '''if [[ "${FAIL_REPLACE:-0}" == 1 && "$1" == */new.app ]]; then exit 1; fi
exec /bin/mv "$@"''',
            'open': '''if [[ "${FAIL_LAUNCH:-0}" == 1 && "$(cat "$1/Contents/MacOS/ctdozlite")" == 0.0.9 ]]; then exit 1; fi
exit 0''',
        }
        for name, body in commands.items():
            path = self.tools / name
            path.write_text('#!/bin/bash\nset -euo pipefail\n' + body + '\n')
            path.chmod(0o755)

    def tearDown(self):
        self.temporary.cleanup()

    def run_update(self, **flags):
        pid = flags.pop('PID', '99999999')
        return subprocess.run(['/bin/bash', str(SCRIPT), pid, str(self.dmg), str(self.app), '0.0.9'],
                              env=dict(self.env, **flags), capture_output=True, text=True, timeout=15)

    def current_version(self):
        return (self.app / 'Contents/MacOS/ctdozlite').read_text()

    def test_success_replaces_app_and_removes_only_private_artifacts(self):
        sibling = self.app.with_name('ctdozlite.app.update-old')
        sibling.mkdir()
        result = self.run_update()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.current_version(), '0.0.9')
        self.assertFalse(self.dmg.exists())
        self.assertTrue(sibling.exists())
        self.assertFalse(list(self.app.parent.glob('.ctdozlite-update.*')))

    def test_preflight_failures_preserve_old_app(self):
        for flag, value in [('FAIL_MOUNT', '1'), ('FAIL_SIGNATURE', '1'), ('FAIL_COPY', '1'), ('FIXTURE_ARCH', 'x86_64')]:
            with self.subTest(flag=flag):
                result = self.run_update(**{flag: value})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.current_version(), '0.0.8')
                self.assertTrue(self.dmg.exists())

    def test_rename_failure_restores_old_app(self):
        self.assertNotEqual(self.run_update(FAIL_REPLACE='1').returncode, 0)
        self.assertEqual(self.current_version(), '0.0.8')

    def test_launch_failure_restores_old_app(self):
        self.assertNotEqual(self.run_update(FAIL_LAUNCH='1').returncode, 0)
        self.assertEqual(self.current_version(), '0.0.8')
        self.assertTrue(self.dmg.exists())

    def test_running_process_is_never_replaced(self):
        self.assertNotEqual(self.run_update(PID=str(os.getpid())).returncode, 0)
        self.assertEqual(self.current_version(), '0.0.8')

    def test_wrong_release_version_is_rejected(self):
        (self.new / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleShortVersionString': '0.0.10'}))
        self.assertNotEqual(self.run_update().returncode, 0)
        self.assertEqual(self.current_version(), '0.0.8')


if __name__ == '__main__':
    unittest.main()
