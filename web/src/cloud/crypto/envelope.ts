import { ed25519 } from "@noble/curves/ed25519.js";
import { canonicalJson } from "./canonical";
export interface SignedEnvelope { version: 1; roomId: string; sender: string; deviceId: string; eventId: string; content: Record<string, unknown>; deviceKeys: { ed25519: string; curve25519: string }; signatures: Record<string, Record<string, string>> }
const decode = (value: string) => Uint8Array.from(atob(value), c => c.charCodeAt(0));
export function unsignedEnvelope(envelope: SignedEnvelope) { const { signatures: _signatures, ...unsigned } = envelope; return unsigned; }
export function verifyEnvelope(envelope: SignedEnvelope, master: string) {
  if (envelope.version !== 1 || !envelope.deviceKeys || envelope.deviceKeys.curve25519 !== envelope.content.sender_key || envelope.deviceId !== envelope.content.device_id) throw new Error("密文设备钥绑定不匹配");
  const signatures = envelope.signatures?.[envelope.sender];
  const masterSignature = signatures?.[`ed25519:${master}`], deviceSignature = signatures?.[`ed25519:${envelope.deviceId}`];
  if (!masterSignature || !deviceSignature) throw new Error("密文缺少账号与设备双签名，不能降级验证");
  const bytes = new TextEncoder().encode(canonicalJson(unsignedEnvelope(envelope)));
  if (!ed25519.verify(decode(masterSignature), bytes, decode(master), { zip215: false }) || !ed25519.verify(decode(deviceSignature), bytes, decode(envelope.deviceKeys.ed25519), { zip215: false })) throw new Error("密文账号或设备签名校验失败");
}
