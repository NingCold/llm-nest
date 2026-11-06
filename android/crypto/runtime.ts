import * as engine from "../../web/src/cloud/crypto/engine";
import { dispatch } from "./operations";
import { context } from "./api";
import "./style.css";
type Args = Record<string, any>;
const account = context.account;
declare global {
  interface Window {
    cryptoCall(id: string, op: string, args: Args): void;
    cryptoRefresh(): void;
    cryptoNotice(message: string): void;
    cryptoSetPeer(account: string): void;
  }
}
window.cryptoCall = (id, op, args) => {
  void dispatch(op, args).then(value => window.NativeCrypto.result(id, JSON.stringify({ value })), error => window.NativeCrypto.result(id, JSON.stringify({ error: { message: error instanceof Error ? error.message : String(error), status: error?.status, code: error?.code } })));
};
const app = document.querySelector<HTMLElement>("#app")!;
let busy = false;
let notice = "";
let targetAccount = account;
let listedAccount = "";
let listedDevices: Args[] = [];
function element<K extends keyof HTMLElementTagNameMap>(tag: K, text = "") {
  const node = document.createElement(tag); node.textContent = text; return node;
}
function button(parent: HTMLElement, label: string, action: () => Promise<unknown> | void, refresh = true) {
  const node = element("button", label);
  node.addEventListener("click", () => { if (busy) return; busy = true; node.disabled = true; void Promise.resolve().then(action).then(() => { notice = ""; }, error => { notice = error instanceof Error ? error.message : String(error); }).finally(() => { busy = false; node.disabled = false; if (refresh || notice) window.cryptoRefresh(); }); });
  parent.append(node); return node;
}
function field(parent: HTMLElement, label: string, type = "text") {
  const wrapper = element("label", label), input = element("input"); input.type = type; input.autocomplete = "off"; wrapper.append(input); parent.append(wrapper); return input;
}
function section(title: string) { const node = element("section"); node.append(element("h2", title)); app.append(node); return node; }
async function render() {
  const state = await dispatch("state", {}) as Args;
  app.replaceChildren(element("h1", "可信设备与恢复"));
  button(app, "完成，返回聊天", () => window.NativeCrypto.done());
  if (notice) { const error = element("p", notice); error.setAttribute("role", "alert"); app.append(error); }
  if (state.locked) {
    const node = section(state.hasDevice ? "解锁本机资料" : "创建独立设备");
    node.append(element("p", "本机口令与登录密码分开，只保护此设备的加密库。不会自动解锁。"));
    const passphrase = field(node, "独立本机口令（至少 8 位）", "password");
    button(node, state.hasDevice ? "解锁本机资料" : "确认创建独立设备", async () => { const value = passphrase.value; passphrase.value = ""; await dispatch("unlock", { passphrase: value, create: !state.hasDevice }); });
    if (state.hasDevice) button(node, "无法解锁？建立另一独立设备", async () => {
      // Native confirm is required; no browser confirmation or silent replacement.
      window.NativeCrypto.recovery("new-device");
    });
    listedDevices = []; return;
  }
  const local = section(state.trusted ? "本机设备 · 已授权" : "本机设备 · 待授权");
  for (const [label, value] of [["设备 ID", state.deviceId], ["Ed25519", state.fingerprint], ["Curve25519", state.curve], ["账号主指纹", state.master ?? "尚未建立"]]) { local.append(element("h3", label), element("code", value)); }
  button(local, "检查授权与密钥更新", () => dispatch("sync", {}));
  button(local, "锁定本机资料", () => dispatch("lock", {}));
  if (!state.master) button(local, "确认建立首次加密身份", () => dispatch("initialize", {}));
  if (state.authorizationAvailable) button(local, "接受已核对设备的加密恢复授权", () => dispatch("accept", {}));
  const recovery = section(state.recoveryConfirmed ? "恢复文件 · 已确认保存" : "恢复文件 · 待保存");
  recovery.append(element("p", "恢复文件包含秘密，持有者可恢复聊天。请妥善保管；平台无法代为恢复。保存完成并单独确认后才允许开启加密。"));
  if (state.hasRecoverySecrets) button(recovery, "生成并保存恢复文件", () => window.NativeCrypto.recovery("export"));
  button(recovery, "从恢复文件授权此设备", () => window.NativeCrypto.recovery("import"));
  const peers = section("核对其他设备");
  peers.append(element("p", "通过独立可信渠道取得并输入完整指纹。服务器提供的名字不能证明设备身份。"));
  const target = field(peers, "要核对的账号 ID"); target.value = targetAccount;
  const listed = element("div"); peers.append(listed);
  target.addEventListener("input", () => { targetAccount = target.value; listedDevices = []; listed.replaceChildren(); });
  button(peers, "查询设备指纹", async () => {
    listedAccount = target.value.trim();
    const result = await dispatch("devices", { account: listedAccount }) as Args;
    listedDevices = result.devices;
  });
    for (const device of listedDevices) {
      const card = element("section"); listed.append(card); card.append(element("h3", `${device.name} · ${device.trusted ? "已信任" : "待核对"}`), element("code", device.deviceId));
      for (const key of ["master", "fingerprint", "curve"]) card.append(element("p", key), element("code", device[key]));
      if (!device.trusted) {
        const master = field(card, "独立渠道取得的账号主指纹"), ed = field(card, "独立渠道取得的 Ed25519"), curve = field(card, "独立渠道取得的 Curve25519");
        const confirmed = field(card, "已通过独立可信渠道核对全部指纹", "checkbox");
        button(card, "确认信任此设备", async () => {
          if (!confirmed.checked || master.value.trim() !== device.master || ed.value.trim() !== device.fingerprint || curve.value.trim() !== device.curve) throw new Error("必须逐项匹配独立渠道取得的全部指纹并确认核对");
          await dispatch("trust", { account: listedAccount, deviceId: device.deviceId, fingerprint: ed.value.trim(), curve: curve.value.trim(), master: master.value.trim() });
          listedDevices = (await dispatch("devices", { account: listedAccount }) as Args).devices;
        });
      }
      if (device.own && device.trusted && device.deviceId !== state.deviceId && state.hasRecoverySecrets) button(card, "向此设备发送恢复授权", () => dispatch("authorize", { deviceId: device.deviceId }));
    }
}
window.cryptoRefresh = () => { void render().catch(error => { app.replaceChildren(element("p", String(error))); }); };
window.cryptoNotice = message => { notice = message; window.cryptoRefresh(); };
window.cryptoSetPeer = peer => { targetAccount = peer || account; listedDevices = []; window.cryptoRefresh(); };
window.addEventListener("llmn-crypto-warning", event => window.cryptoNotice((event as CustomEvent<string>).detail));
window.addEventListener("pagehide", engine.lockCrypto);
window.NativeCrypto.ready();
window.cryptoRefresh();
