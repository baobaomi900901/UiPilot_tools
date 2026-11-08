"""Read-only bundle checks. Never launches a host or an XPC service."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import stat
import subprocess
import sys

HOST_ID = "org.uipilot.probe.xpc-boundary"
SERVICE_ID = HOST_ID + ".service"
MIN_OS = "15.7.4"
SERVICE_NAME = "BoundaryService.xpc"


class InvalidBundle(Exception):
    pass


def require(condition, code):
    if not condition:
        raise InvalidBundle(code)


def tool(*args):
    try:
        result = subprocess.run(args, capture_output=True, timeout=20, check=False)
    except (OSError, subprocess.TimeoutExpired):
        raise InvalidBundle("toolUnavailable") from None
    require(result.returncode == 0, "toolRejected")
    return result


def safe_tree(app):
    require(app.is_dir() and not app.is_symlink(), "bundlePath")
    for parent, directories, files in os.walk(app, followlinks=False):
        for name in directories + files:
            mode = (Path(parent) / name).lstat().st_mode
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), "bundleEntry")


def read_info(bundle, identifier, executable, package_type):
    info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
    require(isinstance(info, dict), "infoShape")
    for key, value in {
        "CFBundleIdentifier": identifier,
        "CFBundleExecutable": executable,
        "CFBundlePackageType": package_type,
        "LSMinimumSystemVersion": MIN_OS,
    }.items():
        require(info.get(key) == value, "infoMismatch")
    binary = bundle / "Contents/MacOS" / executable
    require(binary.is_file() and bool(binary.stat().st_mode & 0o111), "executable")
    return info, binary


def signed_bundle(bundle, binary, identifier, sandboxed):
    tool("/usr/bin/codesign", "--verify", "--strict", "--all-architectures", str(bundle))
    details = tool("/usr/bin/codesign", "-d", "--verbose=4", str(bundle)).stderr.decode()
    reported_id = re.search(r"^Identifier=(.+)$", details, re.M)
    require(reported_id is not None and reported_id.group(1) == identifier, "signingIdentifier")
    encoded = tool("/usr/bin/codesign", "-d", "--entitlements", "-", "--xml", str(bundle)).stdout
    entitlements = plistlib.loads(encoded) if encoded.strip() else {}
    require(isinstance(entitlements, dict), "entitlementShape")
    if sandboxed:
        require(set(entitlements) == {"com.apple.security.app-sandbox"}
                and entitlements["com.apple.security.app-sandbox"] is True,
                "serviceEntitlements")
    else:
        require(entitlements == {}, "hostEntitlements")
    require(tool("/usr/bin/lipo", "-archs", str(binary)).stdout.strip() == b"arm64", "architecture")
    commands = tool("/usr/bin/otool", "-l", str(binary)).stdout.decode()
    versions = re.findall(r"cmd LC_BUILD_VERSION\s+cmdsize \d+\s+platform (\S+)\s+minos (\S+)", commands)
    require(versions == [("1", MIN_OS)], "minimumOS")
    team = re.search(r"^TeamIdentifier=(.+)$", details, re.M)
    return {
        "identifier": identifier,
        "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "architecture": "arm64",
        "minimumOS": MIN_OS,
        "entitlements": entitlements,
        "signatureKind": "adhoc" if "Signature=adhoc" in details else "certificate",
        "reportedTeamIdentifier": team.group(1) if team else None,
    }


def validate(app):
    app = Path(app)
    safe_tree(app)
    services = app / "Contents/XPCServices"
    require(services.is_dir() and {entry.name for entry in services.iterdir()} == {SERVICE_NAME},
            "serviceLayout")
    service = services / SERVICE_NAME
    host_info, host_binary = read_info(app, HOST_ID, "XpcBoundaryProbe", "APPL")
    require(host_info.get("LSBackgroundOnly") is True, "backgroundOnly")
    service_info, service_binary = read_info(service, SERVICE_ID, "BoundaryService", "XPC!")
    xpc = service_info.get("XPCService")
    require(isinstance(xpc, dict) and set(xpc) == {"RunLoopType", "JoinExistingSession"}
            and xpc["RunLoopType"] == "dispatch_main" and xpc["JoinExistingSession"] is False,
            "serviceConfiguration")
    inner = signed_bundle(service, service_binary, SERVICE_ID, True)
    outer = signed_bundle(app, host_binary, HOST_ID, False)
    tool("/usr/bin/codesign", "--verify", "--deep", "--strict", str(app))
    return {"status": "buildValidated", "serviceExecuted": False, "runtimeVerified": False,
            "host": outer, "service": inner}


def main():
    if len(sys.argv) != 2:
        print(json.dumps({"status": "rejected", "reason": "usage"}))
        return 2
    try:
        report = validate(sys.argv[1])
    except InvalidBundle as error:
        report = {"status": "rejected", "reason": str(error)}
    except (OSError, ValueError, TypeError, AttributeError, plistlib.InvalidFileException):
        report = {"status": "rejected", "reason": "invalidBundle"}
    print(json.dumps(report, sort_keys=True))
    return 0 if report["status"] == "buildValidated" else 1


if __name__ == "__main__":
    sys.exit(main())
