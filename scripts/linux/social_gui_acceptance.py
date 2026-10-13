"""Exercise the packaged social UI and actual Tauri IPC with a loopback provider.

Requires WebKitWebDriver and a graphical session (or dbus-run-session/xvfb-run).
All configuration, SQLite and WebKit data stay in a new evidence directory.
This is automated package/GUI coverage, not human IME or GNOME sign-off.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from social_acceptance import Provider, REQUESTS
from webdriver import Driver, wait_for
import http.server


class SocialDriver(Driver):
    def start(self):
        result = self.request("POST", "/session", {"capabilities": {"alwaysMatch": {
            "webkitgtk:browserOptions": {"binary": self.binary},
        }}})
        self.session = result["sessionId"]
        wait_for(lambda: "记录持续保存" in self.text())
        return result["capabilities"]

    def click(self, label):
        wait_for(lambda: self.js("""return [...document.querySelectorAll('button')].some(e =>
            e.getBoundingClientRect().height > 0 && (e.textContent.trim() === arguments[0]
            || e.getAttribute('title') === arguments[0] || e.getAttribute('aria-label') === arguments[0]));""", label))
        selector = (f'//button[normalize-space(.)="{label}" or @title="{label}" '
                    f'or @aria-label="{label}"][not(ancestor::*[@hidden or contains(concat(" ",normalize-space(@class)," ")," is-hidden ")])]')
        element = self.element(selector, "xpath")
        self.call("POST", f"/element/{element}/click", {})

    def fill(self, selector, value):
        wait_for(lambda: self.js("const e=document.querySelector(arguments[0]); return e && !e.disabled && e.getBoundingClientRect().height > 0", selector))
        element = self.element(selector)
        self.call("POST", f"/element/{element}/click", {})
        # Native key events update React's controlled value; WebDriver clear does not.
        self.call("POST", "/actions", {"actions": [{"type": "key", "id": "keyboard", "actions": [
            {"type": "keyDown", "value": "\ue009"}, {"type": "keyDown", "value": "a"},
            {"type": "keyUp", "value": "a"}, {"type": "keyUp", "value": "\ue009"},
            {"type": "keyDown", "value": "\ue003"}, {"type": "keyUp", "value": "\ue003"},
        ]}]})
        wait_for(lambda: self.js("return document.querySelector(arguments[0]).value", selector) == "")
        if value:
            self.call("POST", f"/element/{element}/value", {"text": value})
        wait_for(lambda: self.js("return document.querySelector(arguments[0]).value", selector) == value)

    def ipc(self, name, **args):
        result = self.call("POST", "/execute/async", {"script": """
            const done = arguments[arguments.length - 1];
            window.__TAURI_INTERNALS__.invoke(arguments[0], arguments[1]).then(
                value => done({ok: true, value}), error => done({ok: false, error}));
            """, "args": [name, args]})
        if not result["ok"]:
            raise IpcError(result["error"])
        return result.get("value")

    def open_chat(self, title):
        wait_for(lambda: self.js("return [...document.querySelectorAll('button.conversation-list-item strong')].some(e => e.textContent === arguments[0])", title))
        element = self.element(f'//button[contains(@class,"conversation-list-item")][.//strong[text()="{title}"]]', "xpath")
        self.call("POST", f"/element/{element}/click", {})
        wait_for(lambda: self.js("return !!document.querySelector('textarea[aria-label=\"消息输入框\"]')"))


class IpcError(Exception):
    def __init__(self, body):
        super().__init__(str(body))
        self.body = body


def checks(driver, work, passed):
    def check(name):
        passed.append(name)
        print("PASS:", name, flush=True)

    def command(**values):
        return dict(requestId=str(uuid.uuid4()), **values)

    def rejected(code, name, **args):
        try:
            driver.ipc(name, **args)
            raise AssertionError("command unexpectedly succeeded: " + name)
        except IpcError as error:
            assert error.body["code"] == code and error.body["message"], error.body

    def history(cid):
        return driver.ipc("social_messages", id=cid, before=None)["messages"]

    def send(cid, text, mentions=None):
        return driver.ipc("social_send", id=cid, input=command(text=text, mentions=mentions or [], replyTo=None))

    def settled(cid):
        values = driver.ipc("social_plans", id=cid)
        return values if values and all(v["status"] not in ("queued", "running") for v in values) else None

    initial = driver.ipc("social_init")
    assert len(initial["accounts"]) == 1 and not initial["conversations"]
    assert not driver.js("return !!document.querySelector('[data-tauri-drag-region]')")
    driver.screenshot("01-social-startup")
    actors = []
    for index, name in enumerate(("验收甲", "验收乙", "验收丙")):
        managed = driver.ipc("social_create_ai", input=command(name=name, bio="公开简介", persona="内部设定-" + name,
            binding={"providerId": "test", "modelId": "chat", "temperature": index * .2, "maxTokens": 1024}))
        actors.append(managed["account"])
    assert all("persona" not in a and "binding" not in a for a in driver.ipc("social_init")["accounts"])
    rejected("model_unavailable", "social_create_ai", input=command(name="无效模型", bio="", persona="",
        binding={"providerId": "test", "modelId": "missing"}))
    assert len(driver.ipc("social_managed_accounts")) == 3
    private = driver.ipc("social_open_direct", id=actors[0]["id"], input=command())
    assert private == driver.ipc("social_open_direct", id=actors[0]["id"], input=command())
    driver.open_chat("验收甲")
    # WebKitWebDriver's key insertion drops astral characters; cover emoji via IPC.
    driver.fill('textarea[aria-label="消息输入框"]', "中文社交验收")
    driver.click("发送消息")
    wait_for(lambda: any(m["text"] == "回应：中文社交验收" and m["status"] == "committed" for m in history(private)))
    wait_for(lambda: "回应：中文社交验收" in driver.text())
    send(private, "IPC emoji🙂")
    wait_for(lambda: history(private)[-1]["text"] == "回应：IPC emoji🙂" and history(private)[-1]["status"] == "committed")
    wait_for(lambda: "回应：IPC emoji🙂" in driver.text())
    driver.screenshot("02-private-ipc")
    check("packaged social startup, public/private isolation, strict models, unique private chat and Unicode GUI/IPC reply")

    driver.fill('textarea[aria-label="消息输入框"]', "保留未发送草稿")
    driver.click("设置")
    driver.click("外观")
    driver.click("浅色")
    wait_for(lambda: not driver.js("return document.documentElement.classList.contains('dark')"))
    driver.screenshot("03-settings-light")
    driver.click("深色")
    wait_for(lambda: driver.js("return document.documentElement.classList.contains('dark')"))
    driver.click("返回聊天")
    wait_for(lambda: driver.js("return document.querySelector('textarea').getBoundingClientRect().height > 0"))
    assert driver.js("return document.querySelector('textarea').value") == "保留未发送草稿"
    driver.screenshot("04-chat-dark")
    check("appearance settings and draft retained across navigation")

    group = driver.ipc("social_create_group", input=command(title="IPC 验收群", memberIds=[a["id"] for a in actors]))
    send(group, "queue-three", [a["id"] for a in actors])
    wait_for(lambda: settled(group))
    assert [m["senderId"] for m in history(group) if m["runId"]] == [a["id"] for a in actors]
    driver.open_chat("IPC 验收群")
    wait_for(lambda: "回应：queue-three" in driver.text())
    driver.screenshot("05-group-ipc")
    rejected("not_authorized", "social_account_view_messages", id=actors[1]["id"], conversation=private, before=None)
    original = history(private)[0]
    rejected("revision_conflict", "social_revise_message", id=original["id"],
        input=command(expectedRevision=str(int(original["revision"]) + 1), text="过期修改"))
    assert history(private)[0] == original
    check("three-AI FIFO group replies, scoped account viewpoint and stale revision rejection through actual IPC")

    driver.open_chat("验收甲")
    driver.fill('textarea[aria-label="消息输入框"]', "")
    for text, expected in (("provider-error", "provider_error"), ("eof", "unexpected_eof"), ("tool", "tool_not_allowed")):
        send(private, text)
        def interrupted():
            recent = history(private)[-2:]
            return len(recent) == 2 and recent[0]["text"] == text and recent[1]["status"] == "interrupted"
        wait_for(interrupted)
        detail = driver.ipc("social_run_details", id=history(private)[-1]["runId"])
        assert detail["error"]["code"] == expected, detail
        if text == "provider-error":
            wait_for(lambda: driver.js("return [...document.querySelectorAll('button.conversation-list-item')].some(e => e.textContent.includes('验收甲') && e.textContent.includes('回复已中断'))"))
    send(private, "slow-cancel")
    wait_for(lambda: history(private)[-1]["status"] == "generating" and bool(history(private)[-1]["text"]))
    driver.ipc("social_cancel_run", id=history(private)[-1]["runId"])
    wait_for(lambda: history(private)[-1]["status"] == "interrupted")
    driver.screenshot("06-interrupted")
    check("provider error, early EOF, prohibited tools and cancellation preserve interrupted terminal records")

    database = work / "sessions/social.sqlite3"
    before = history(private)
    with sqlite3.connect(database) as db:
        db.execute("CREATE TRIGGER fixture_ipc_failure BEFORE INSERT ON messages BEGIN SELECT RAISE(FAIL,'synthetic failure'); END")
    try:
        driver.fill('textarea[aria-label="消息输入框"]', "保存失败后重试")
        driver.click("发送消息")
        wait_for(lambda: "消息存储失败" in driver.text())
        assert history(private) == before
        assert driver.js("return document.querySelector('textarea').value") == "保存失败后重试"
        driver.screenshot("07-save-failure")
    finally:
        with sqlite3.connect(database) as db:
            db.execute("DROP TRIGGER fixture_ipc_failure")
    driver.click("发送消息")
    wait_for(lambda: history(private)[-1]["text"] == "回应：保存失败后重试" and history(private)[-1]["status"] == "committed")
    check("SQLite write failure is visible, leaves no message or lost draft, and the same GUI retry succeeds")

    size = driver.js("return {width:innerWidth,height:innerHeight}")
    driver.call("POST", "/window/rect", {"width": 800, "height": 600})
    wait_for(lambda: driver.js("return innerWidth === 800 && innerHeight === 600"))
    assert driver.js("""const r=document.querySelector('textarea').getBoundingClientRect();
        return r.width > 0 && r.left >= 0 && r.right <= innerWidth && r.bottom <= innerHeight
            && document.documentElement.scrollWidth === innerWidth;""")
    driver.screenshot("08-minimum-window")
    driver.call("POST", "/window/rect", size)
    check("system-decorated Linux window and visible composer at 800x600")

    send(private, "crash")
    def saved_fragment():
        with sqlite3.connect(database) as db:
            return db.execute("SELECT count(*) FROM messages WHERE status='generating' AND json_extract(payload,'$.body[0].text') != ''").fetchone()[0] == 1
    wait_for(saved_fragment)
    driver.crash_app()
    driver.start()
    recovered = history(private)
    assert recovered[-1]["status"] == "interrupted" and recovered[-1]["text"]
    driver.open_chat("验收甲")
    wait_for(lambda: "本次回复已中断" in driver.text())
    driver.screenshot("09-crash-recovery")
    driver.close()
    driver.start()
    assert history(private) == recovered
    with sqlite3.connect(database) as db:
        assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    check("SIGKILL restores the saved fragment once, keeps history and does not replay generation")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--driver", type=Path)
    parser.add_argument("--manual", action="store_true",
                        help="Open an isolated social mock-provider fixture until the app closes; no automatic sign-off")
    parser.add_argument("--backend", choices=("x11", "wayland"), default="x11")
    parser.add_argument("--scale", choices=(1, 2), type=int, default=1,
                        help="GTK integer scale; not GNOME fractional scaling")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    work = args.output.resolve() if args.output else Path(tempfile.mkdtemp(prefix="llmn-social-gui-"))
    if args.output:
        work.mkdir(parents=True, exist_ok=False)
    if not args.manual and not args.driver:
        parser.error("--driver is required unless --manual is used")
    binary = args.binary.resolve(strict=True)
    executable = args.driver.resolve(strict=True) if args.driver else None
    provider = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    config = work / "llmn.toml"
    config.write_text(f'[providers.test]\nprotocol="openai"\nbase_url="http://127.0.0.1:{provider.server_port}/v1"\n'
        'api_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat-wire"\ncontext_window=8192\nmax_tokens=1024\n', encoding="utf-8")
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    env = dict(os.environ, LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(work / "sessions"),
        XDG_CONFIG_HOME=str(work / "config"), XDG_DATA_HOME=str(work / "data"),
        XDG_CACHE_HOME=str(work / "cache"), TAURI_WEBVIEW_AUTOMATION="true",
        GDK_BACKEND=args.backend, GDK_SCALE=str(args.scale))
    if args.manual:
        env.pop("TAURI_WEBVIEW_AUTOMATION", None)
    driver = SocialDriver(port, binary, work)
    report = {"binary": binary.name, "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "passed": [],
        "requested_backend": args.backend, "gtk_scale": args.scale,
        "classification": "manual fixture; no sign-off" if args.manual else "automated GUI/IPC; no human desktop sign-off",
        "manual_pending": ["IME candidates and Enter", "emoji keyboard input", "GNOME fractional scaling and launchers"]}
    print("Evidence:", work, flush=True)
    with (work / "driver.log").open("w") as log:
        launch = [str(binary)] if args.manual else [str(executable), f"--port={port}"]
        process = subprocess.Popen(launch, env=env, cwd=work,
            stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        driver.process_group = process.pid
        try:
            if args.manual:
                print("独立社交验收环境：创建 AI 时选择 test/chat；普通消息回复中文，slow 测慢流，provider-error 测失败。\n"
                      "请检查 GNOME 入口、系统窗口、中文候选与回车、剪贴板及显示缩放。关闭窗口结束；不自动判定人工通过。", flush=True)
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
            report["viewport"] = driver.js("return {width:innerWidth,height:innerHeight,dpr:devicePixelRatio}")
            checks(driver, work, report["passed"])
            report["result"] = "passed"
        except BaseException as error:
            report["result"], report["error"] = "failed", str(error)
            if driver.session:
                try:
                    driver.screenshot("failure")
                    (work / "failure-text.txt").write_text(driver.text(), encoding="utf-8")
                    (work / "failure-input.json").write_text(json.dumps(driver.js("return [...document.querySelectorAll('textarea')].map(e => ({value:e.value,focused:e===document.activeElement}))"), ensure_ascii=False), encoding="utf-8")
                except (RuntimeError, OSError):
                    pass
            raise
        finally:
            try:
                driver.close()
            finally:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=5)
                except ProcessLookupError:
                    pass
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                provider.shutdown()
                provider.server_close()
                (work / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
                (work / "requests.json").write_text(json.dumps(REQUESTS, ensure_ascii=False, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
