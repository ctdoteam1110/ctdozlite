#!/usr/bin/env python3
"""Disposable bundle contracts using built helpers, never the installed application."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BUILD = Path(os.environ.get("NATIVE_BUILD_DIR", ROOT / "native/macos/build"))
spec = importlib.util.spec_from_file_location("bundle_validator", ROOT / "packaging/macos/validate-native-bundle.py")
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class NativePackagingTests(unittest.TestCase):
    def test_public_build_has_no_download_or_asset_opt_in(self):
        script = (ROOT / "native/macos/build-native-helpers.sh").read_text()
        self.assertNotIn("prepare-history-assets", script)
        self.assertIn("HistoryCodec", script)
        workflow = (ROOT / ".github/workflows/ci-release.yml").read_text()
        self.assertIn("bash packaging/package-local.sh", workflow)
        packaging = (ROOT / "packaging/package-local.sh").read_text()
        self.assertIn('env -u HISTORY_CODEC_ASSETS SWIFT_TARGETS=', packaging)
        self.assertIn('Contents/MacOS/ctdozlite', packaging)
        self.assertNotIn('zalo-rust', packaging)
        self.assertIn('"aws-lc-sys": "AWS-LC-LICENSE"', packaging)
        self.assertLess(packaging.index("validate-native-bundle.py"), packaging.index('codesign --force --sign -'))

    def test_asset_download_requires_explicit_local_use(self):
        result = subprocess.run(["python3", str(ROOT / "native/macos/prepare-history-assets.py")], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--local-use", result.stderr)

    def test_relocated_bundle_architecture_matrix(self):
        for arches in [set(group.split(",")) for group in os.environ.get("NATIVE_TEST_ARCHES", "arm64;x86_64;arm64,x86_64").split(";")]:
            with self.subTest(arches=arches), tempfile.TemporaryDirectory(prefix="native-package-test-") as temporary:
                app = Path(temporary) / "Synthetic.app"
                paths = dict(validator.HELPERS)
                # A helper stands in for the main executable for packaging validation only.
                paths["MainFixture"] = "MacOS/ctdozlite"
                for name, path in paths.items():
                    binary = BUILD / ("EmojiPreview" if name == "MainFixture" else name)
                    destination = app / "Contents" / path
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    if len(arches) == 1:
                        actual = set(validator.output("lipo", "-archs", str(binary)).split())
                        if actual == arches:
                            shutil.copy2(binary, destination)
                        else:
                            subprocess.run(["lipo", str(binary), "-thin", next(iter(arches)), "-output", str(destination)], check=True)
                    else:
                        shutil.copy2(binary, destination)
                    destination.chmod(0o755)
                    subprocess.run(["codesign", "--force", "--sign", "-", str(destination)], capture_output=True, check=True)
                licenses = app / "Contents/Resources/ThirdPartyLicenses"
                licenses.mkdir()
                shutil.copy2(BUILD / "zstd-1.5.7/arm64/source/LICENSE", licenses / "zstd-LICENSE")
                for notice in ("AWS-LC-LICENSE", "aws-lc-rs-LICENSE"):
                    (licenses / notice).write_text("fixture notice\n")
                validator.validate(app, arches)
                with self.assertRaisesRegex(AssertionError, "Architecture mismatch"):
                    validator.validate(app, {"invalid-architecture"})
                assets = app / "Contents/Resources/HistoryCodecAssets"
                assets.mkdir()
                with self.assertRaisesRegex(AssertionError, "external codec assets"):
                    validator.validate(app, arches)


if __name__ == "__main__":
    unittest.main()
