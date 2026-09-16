"""Exercise an existing Linux desktop package through WebKit WebDriver.

Requires a running graphical session and a matching WebKitWebDriver (Ubuntu:
webkit2gtk-driver / webkitgtk-webdriver). No Selenium, model keys or Rust build.
Pass the installed executable or the AppImage itself, not its extracted ELF.
All configuration, sessions and WebKit data stay in a fresh evidence directory.
This is automated GUI coverage, not human IME/desktop integration acceptance.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("acceptance", ROOT / "scripts/acceptance.py")
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


def wait_for(predicate, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError("condition timed out")


class Driver:
    def __init__(self, port, binary, work):
        self.url = f"http://127.0.0.1:{port}"
        self.binary, self.work, self.session = str(binary), work, None

    def request(self, method, path, data=None):
        body = None if data is None else json.dumps(data).encode()
        request = urllib.request.Request(self.url + path, data=body, method=method,
                                         headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=35) as response:
                return json.load(response)["value"]
        except urllib.error.HTTPError as error:
            raise RuntimeError(error.read().decode()) from error

    def call(self, method, path, data=None):
        return self.request(method, f"/session/{self.session}{path}", data)

    def start(self):
        result = self.request("POST", "/session", {"capabilities": {"alwaysMatch": {
            "webkitgtk:browserOptions": {"binary": self.binary},
        }}})
        self.session = result["sessionId"]
        wait_for(lambda: "桌面模式" in self.text())
        return result["capabilities"]

    def close(self):
        if self.session:
            try:
                self.call("DELETE", "")
            finally:
                self.session = None

    def js(self, script, *args):
        return self.call("POST", "/execute/sync", {"script": script, "args": list(args)})

    def text(self):
        return self.js("return document.body.innerText")

    def element(self, value, using="css selector"):
        return self.call("POST", "/element", {"using": using, "value": value})[
            "element-6066-11e4-a52e-4f735466cecf"]

    def click(self, label):
        # Visible page only: settings and chat deliberately stay mounted.
        selector = (f'//button[normalize-space(.)="{label}" or @title="{label}" '
                    f'or @aria-label="{label}"][not(ancestor::*[@hidden])]')
        element = self.element(selector, "xpath")
        self.call("POST", f"/element/{element}/click", {})

    def fill(self, selector, value):
        wait_for(lambda: self.js("""const e = document.querySelector(arguments[0]);
            return e && !e.disabled && e.getBoundingClientRect().height > 0;""", selector))
        element = self.element(selector)
        self.call("POST", f"/element/{element}/clear", {})
        if value:
            self.call("POST", f"/element/{element}/value", {"text": value})
        wait_for(lambda: self.js("return document.querySelector(arguments[0]).value", selector) == value)

    def screenshot(self, name):
        self.call("POST", "/execute/async", {"script": """const done = arguments[arguments.length - 1];
            requestAnimationFrame(() => requestAnimationFrame(() => done()));""", "args": []})
        (self.work / f"{name}.png").write_bytes(base64.b64decode(self.call("GET", "/screenshot")))

    def streaming(self):
        return self.js("return !!document.querySelector(arguments[0])", 'button[title="停止生成"]')

    def crash_app(self):
        # Target only the app in this driver's private process group.
        candidates = []
        for path in Path("/proc").glob("[0-9]*/comm"):
            try:
                pid = int(path.parent.name)
                if path.read_text().strip() == "LLM-Nest" and os.getpgid(pid) == self.process_group:
                    candidates.append(pid)
            except (OSError, ProcessLookupError):
                pass
        assert len(candidates) == 1, candidates
        os.kill(candidates[0], signal.SIGKILL)
        try:
            self.close()
        except RuntimeError:
            pass  # The driver reports the page crash after SIGKILL.


def run_checks(driver, work, passed, failures):
    def check(name):
        passed.append(name)
        print("PASS:", name, flush=True)

    def records():
        return [json.loads(p.read_text()) for p in (work / "sessions").glob("*.json")]

    def latest():
        return max(records(), key=lambda r: r["updated_at"])

    def send(value, status="succeeded"):
        count = len(latest()["messages"]) if records() else 0
        driver.fill("textarea", value)
        driver.click("发送")
        wait_for(lambda: len(latest()["messages"]) > count and latest()["run"]["status"] == status)
        wait_for(lambda: not driver.streaming())

    assert (work / "sessions/.llmn.lock").is_file()
    if driver.js("return !!document.querySelector('[data-tauri-drag-region]')"):
        failures.append("Linux settings retains custom titlebar drag regions")
    driver.screenshot("01-startup")
    check("package startup and desktop IPC")
    driver.click("新建对话")
    wait_for(lambda: bool(records()))
    send("中文自动化验收")  # WebDriver text insertion is not an IME candidate test.
    assert latest()["messages"][-1]["content"] == "你好，验收🙂"
    assert latest()["messages"][-1]["reasoning"] == "验收思考🙂"
    assert latest()["messages"][-1]["usage"]["total_tokens"] == 15
    driver.screenshot("02-unicode-chat")
    check("Unicode input, streamed reply, reasoning and usage persisted")

    driver.click("复制全文")
    driver.fill("textarea", "")
    driver.call("POST", f"/element/{driver.element('textarea')}/click", {})
    driver.call("POST", "/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": [
        {"type": "keyDown", "value": "\ue009"}, {"type": "keyDown", "value": "v"},
        {"type": "keyUp", "value": "v"}, {"type": "keyUp", "value": "\ue009"},
    ]}]})
    wait_for(lambda: driver.js("return document.querySelector('textarea').value") == "你好，验收🙂")
    driver.screenshot("02-clipboard-paste")
    check("copy reply and Ctrl+V round-trip including Unicode and emoji")

    driver.fill("textarea", "保留未发送草稿")
    driver.click("设置")
    driver.click("生成参数")
    driver.fill('input[aria-label="温度"]', "0.3")
    driver.fill('input[aria-label="最大输出 tokens"]', "1234")
    driver.click("保存生成设置")
    wait_for(lambda: "已保存" in driver.text())
    driver.screenshot("03-settings")
    driver.click("外观")
    driver.click("浅色")
    assert not driver.js("return document.documentElement.classList.contains('dark')")
    driver.screenshot("04-light")
    driver.click("深色")
    assert driver.js("return document.documentElement.classList.contains('dark')")
    driver.screenshot("05-dark")
    driver.click("返回聊天")
    assert driver.js("return document.querySelector('textarea').value") == "保留未发送草稿"
    send("settings")
    assert fixture.REQUESTS[-1]["temperature"] == 0.3
    assert fixture.REQUESTS[-1]["max_tokens"] == 1234
    check("settings saved and used, theme switching, draft retained across settings")

    driver.fill("textarea", "slow")
    driver.click("发送")
    wait_for(driver.streaming)
    wait_for(lambda: bool((latest()["run"].get("partial") or {}).get("content")))
    driver.click("停止生成")
    wait_for(lambda: latest()["run"]["status"] == "cancelled")
    wait_for(lambda: "已停止生成" in driver.text())
    assert latest()["messages"][-1]["interruption"] == "cancelled"
    assert latest()["messages"][-1]["content"].startswith("你好，验收🙂")
    driver.screenshot("06-cancelled")
    send("error", "failed")
    wait_for(lambda: "acceptance rate limit" in driver.text())
    driver.screenshot("07-error")
    send("retry")
    check("cancel persists partial reply, provider error and subsequent retry")

    # Wait for asynchronous GTK/WebKit layout after the native resize reply.
    original_size = driver.js("return {width:innerWidth, height:innerHeight}")
    driver.call("POST", "/window/rect", {"width": 800, "height": 600})
    wait_for(lambda: driver.js("return innerWidth === 800 && innerHeight === 600"))
    assert driver.js("""const r = document.querySelector('textarea').getBoundingClientRect();
        return r.width > 0 && r.left >= 0 && r.right <= innerWidth && r.bottom <= innerHeight;""")
    driver.screenshot("08-minimum-window")
    # At GTK scale 2 a 1200x800 logical window can exceed the monitor work area.
    driver.call("POST", "/window/rect", original_size)
    # GTK/XWayland can round an odd physical work-area edge at scale 2.
    wait_for(lambda: driver.js("return Math.abs(innerWidth - arguments[0].width) <= 1 && Math.abs(innerHeight - arguments[0].height) <= 1", original_size))
    (work / "resize.json").write_text(json.dumps({"initial": original_size,
        "restored": driver.js("return {width:innerWidth, height:innerHeight, dpr:devicePixelRatio}")}, indent=2))
    check("native resize, composer visible at 800x600 and restored to initial size")

    before = latest()["messages"]
    driver.close()
    driver.start()
    wait_for(lambda: "你好，验收🙂" in driver.text())
    assert latest()["messages"] == before
    assert driver.js("return document.documentElement.classList.contains('dark')")
    driver.click("设置")
    driver.click("生成参数")
    assert driver.js("return document.querySelector('[aria-label=温度]').value") == "0.3"
    assert driver.js("return document.querySelector('[aria-label=\"最大输出 tokens\"]').value") == "1234"
    driver.screenshot("09-restarted-settings")
    driver.click("返回聊天")
    driver.screenshot("10-restarted-history")
    check("process restart restores messages, cancelled state, theme and settings")

    driver.fill("textarea", "crash")
    driver.click("发送")
    wait_for(lambda: bool((latest()["run"].get("partial") or {}).get("content")))
    driver.crash_app()
    driver.start()
    wait_for(lambda: latest()["run"]["status"] == "interrupted")
    assert latest()["messages"][-1]["content"].startswith("你好，验收🙂")
    assert latest()["messages"][-1]["reasoning"] == "验收思考🙂"
    wait_for(lambda: "recovered from checkpoint" in driver.text() and "你好，验收🙂" in driver.text())
    recovered = latest()["messages"]
    driver.screenshot("11-crash-recovery")
    driver.close()
    driver.start()
    wait_for(lambda: "recovered from checkpoint" in driver.text())
    assert latest()["messages"] == recovered
    check("SIGKILL recovers checkpoint once without replay")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--driver", type=Path)
    parser.add_argument("--manual", action="store_true",
                        help="Open an isolated mock-provider fixture until the app closes; no automatic sign-off")
    parser.add_argument("--output", type=Path, help="New directory; refuses to overwrite existing evidence")
    parser.add_argument("--backend", choices=("x11", "wayland"), default="x11")
    parser.add_argument("--scale", choices=(1, 2), type=int, default=1,
                        help="GTK application integer scale; not GNOME fractional scaling")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if not args.manual and not args.driver:
        parser.error("--driver is required unless --manual is used")
    executable = args.driver.resolve(strict=True) if args.driver else None
    if args.output:
        work = args.output.resolve()
        work.mkdir(parents=True, exist_ok=False)
    else:
        work = Path(tempfile.mkdtemp(prefix="llmn-linux-gui-"))
    provider = fixture.http.server.ThreadingHTTPServer(("127.0.0.1", 0), fixture.Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    config = work / "llmn.toml"
    config.write_text(f'[providers.test]\nprotocol="openai"\n'
                      f'base_url="http://127.0.0.1:{provider.server_port}/v1"\n'
                      'api_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat"\n')
    port = fixture.free_port()
    env = dict(os.environ, LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(work / "sessions"),
               XDG_CONFIG_HOME=str(work / "config"), XDG_DATA_HOME=str(work / "data"),
               XDG_CACHE_HOME=str(work / "cache"), TAURI_WEBVIEW_AUTOMATION="true",
               GDK_BACKEND=args.backend, GDK_SCALE=str(args.scale))
    env.pop("APPIMAGE_EXTRACT_AND_RUN", None)  # Exercise the real FUSE entry point.
    if args.manual:
        env.pop("TAURI_WEBVIEW_AUTOMATION", None)
    report = {"binary": binary.name, "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "requested_backend": args.backend, "gtk_scale": args.scale, "passed": [], "failures": [],
              "classification": "manual fixture; no sign-off" if args.manual else "automated GUI; no human sign-off",
              "manual_pending": ["launcher/menu icon", "IME candidates and Enter",
                                 "GNOME 100/150/200% scaling", "native window drag/controls",
                                 "cross-application clipboard", "Ubuntu 24.04 desktop"]}
    driver = Driver(port, binary, work)
    print(f"Evidence: {work}", flush=True)
    with (work / "driver.log").open("w") as log:
        command = [str(binary)] if args.manual else [str(executable), f"--port={port}"]
        process = subprocess.Popen(command, env=env, cwd=work,
                                   stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        driver.process_group = process.pid
        try:
            if args.manual:
                print("本地模拟 provider：普通消息回复中文；slow 测取消；error 测失败；tool 测 add。\n"
                      "请手工验证图标、系统标题栏、中文候选/回车、剪贴板、显示缩放。关闭应用结束。\n"
                      "此模式仅准备验收环境，不自动判定人工通过。", flush=True)
                report["exit_code"] = process.wait()
                report["result"] = "manual results not recorded"
                return
            def ready():
                if process.poll() is not None:
                    raise RuntimeError("WebKitWebDriver exited; see driver.log")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=.2):
                        return True
                except OSError:
                    return False
            wait_for(ready)
            report["capabilities"] = driver.start()
            report["viewport"] = driver.js("return {width:innerWidth, height:innerHeight, dpr:devicePixelRatio}")
            run_checks(driver, work, report["passed"], report["failures"])
            assert not report["failures"], report["failures"]
            report["result"] = "passed"
        except BaseException as error:
            report["result"], report["error"] = "failed", str(error)
            try:
                driver.screenshot("failure")
                (work / "failure-text.txt").write_text(driver.text())
            except Exception:
                pass
            raise
        finally:
            try:
                driver.close()
            except Exception:
                pass
            try:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=5)
            except ProcessLookupError:
                pass
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            provider.shutdown()
            (work / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
            (work / "requests.json").write_text(json.dumps(fixture.REQUESTS, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
