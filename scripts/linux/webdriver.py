"""Shared WebKit driver primitives for isolated social desktop QA."""
import base64
import json
import os
from pathlib import Path
import signal
import time
import urllib.request
import urllib.error

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

    def close(self):
        if self.session:
            try:
                if self.app_pids():
                    self.js("window.__TAURI_INTERNALS__.invoke('plugin:window|close', {label:'main'}); return true")
                    wait_for(lambda: not self.app_pids(), timeout=25)
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

    def app_pids(self):
        # Target only the app in this driver's private process group.
        candidates = []
        for path in Path("/proc").glob("[0-9]*/comm"):
            try:
                pid = int(path.parent.name)
                state = (path.parent / "stat").read_text().rsplit(") ", 1)[1].split()[0]
                if path.read_text().strip() == "LLM-Nest" and state != "Z" and os.getpgid(pid) == self.process_group:
                    candidates.append(pid)
            except (OSError, ProcessLookupError):
                pass
        return candidates

    def crash_app(self):
        candidates = self.app_pids()
        assert len(candidates) == 1, candidates
        os.kill(candidates[0], signal.SIGKILL)
        try:
            self.call("DELETE", "")
        except RuntimeError:
            pass  # The driver reports the page crash after SIGKILL.
        finally:
            self.session = None
