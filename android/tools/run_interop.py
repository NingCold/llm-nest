"""Run the Android integration tests on an explicitly named, disposable emulator.
Build the test APKs using the fixture's res overlay before invoking this script.
"""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
from urllib.parse import urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True, help="Dedicated emulator, e.g. emulator-5554")
    parser.add_argument("--fixture", type=Path, required=True, help="Directory created by interop_fixture.py")
    args = parser.parse_args()
    if not re.fullmatch(r"emulator-[0-9]+", args.serial):
        parser.error("Use a disposable Android emulator, not a daily-use phone.")
    fixture = json.loads((args.fixture / "fixture.json").read_text(encoding="utf-8"))
    origin = urlsplit(fixture["origin"])
    if origin.scheme != "https" or origin.hostname != "127.0.0.1" or not origin.port:
        parser.error("This runner only accepts the local TLS fixture.")
    android = Path(__file__).resolve().parents[1]
    prefix = ["adb", "-s", args.serial]
    socket = f"tcp:{origin.port}"
    apks = android / "app/build/outputs/apk"
    subprocess.run([*prefix, "reverse", socket, socket], check=True)
    try:
        for apk in [apks / "debug/app-debug.apk", apks / "androidTest/debug/app-debug-androidTest.apk"]:
            subprocess.run([*prefix, "install", "-r", str(apk)], check=True)
        # The plaintext UI test precedes activation of encryption on the same private conversation.
        for cls in ["AndroidChatUiTest", "CryptoInteropTest", "DeviceRecoveryUiTest"]:
            argv = ["am", "instrument", "-w", "-r", "-e", "class", f"com.llmnest.android.{cls}"]
            test_args = {key: fixture[key] for key in ("origin", "emailA", "passwordA", "emailB", "passwordB")}
            if cls == "DeviceRecoveryUiTest":
                test_args = {"origin": fixture["origin"], "emailA": fixture["emailRecovery"], "passwordA": fixture["passwordRecovery"]}
            for key, value in test_args.items():
                argv += ["-e", key, value]
            argv.append("com.llmnest.android.debug.test/androidx.test.runner.AndroidJUnitRunner")
            result = subprocess.run([*prefix, "shell", shlex.join(argv)], capture_output=True, encoding="utf-8", errors="replace")
            output = result.stdout + result.stderr
            for key, value in test_args.items():
                if key.startswith("password"):
                    output = output.replace(value, "[redacted]")
            if result.returncode or not re.search(r"OK \(\d+ tests?\)", output):
                raise SystemExit(f"{cls} failed:\n{output}")
            print(cls + ": passed")
    finally:
        subprocess.run([*prefix, "reverse", "--remove", socket], check=True)


if __name__ == "__main__":
    main()
