"""Mutate and re-sign throwaway bundles; no executable is launched."""
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest

from validator import HOST_ID, SERVICE_ID, SERVICE_NAME, InvalidBundle, validate


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="uipilot-xpc-validation-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.app = self.root / "Probe.app"
        shutil.copytree(os.environ["UIPILOT_XPC_TEST_APP"], self.app)
        self.service = self.app / "Contents/XPCServices" / SERVICE_NAME

    def sign(self, bundle, identifier, entitlements):
        path = self.root / "entitlements.plist"
        path.write_bytes(plistlib.dumps(entitlements))
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--identifier", identifier,
                        "--entitlements", str(path), str(bundle)], check=True, capture_output=True)

    def sign_outer(self):
        self.sign(self.app, HOST_ID, {})

    def rejects(self, reason):
        with self.assertRaisesRegex(InvalidBundle, "^" + reason + "$"):
            validate(self.app)

    def test_valid_build_is_not_runtime_proof(self):
        report = validate(self.app)
        self.assertEqual(report["status"], "buildValidated")
        self.assertFalse(report["serviceExecuted"])
        self.assertFalse(report["runtimeVerified"])

    def test_extra_final_service_entitlement_rejected_after_resigning(self):
        self.sign(self.service, SERVICE_ID, {"com.apple.security.app-sandbox": True,
                                          "com.apple.security.network.client": True})
        self.sign_outer()
        self.rejects("serviceEntitlements")

    def test_host_entitlement_rejected_after_resigning(self):
        self.sign(self.app, HOST_ID, {"com.apple.security.network.client": True})
        self.rejects("hostEntitlements")

    def test_integer_one_is_not_a_boolean_sandbox_entitlement(self):
        self.sign(self.service, SERVICE_ID, {"com.apple.security.app-sandbox": 1})
        self.sign_outer()
        self.rejects("serviceEntitlements")

    def test_signing_identifier_cannot_differ_from_info(self):
        self.sign(self.service, SERVICE_ID + ".wrong", {"com.apple.security.app-sandbox": True})
        self.sign_outer()
        self.rejects("signingIdentifier")

    def test_wrong_info_identifier_is_rejected_even_with_valid_signature(self):
        path = self.service / "Contents/Info.plist"
        info = plistlib.loads(path.read_bytes())
        info["CFBundleIdentifier"] = SERVICE_ID + ".wrong"
        path.write_bytes(plistlib.dumps(info))
        self.sign(self.service, SERVICE_ID, {"com.apple.security.app-sandbox": True})
        self.sign_outer()
        self.rejects("infoMismatch")

    def test_symlink_and_extra_service_are_rejected(self):
        (self.app / "Contents/link").symlink_to("Info.plist")
        self.rejects("bundleEntry")
        (self.app / "Contents/link").unlink()
        (self.app / "Contents/XPCServices/Unexpected.xpc").mkdir()
        self.rejects("serviceLayout")


if __name__ == "__main__":
    unittest.main(verbosity=2)
