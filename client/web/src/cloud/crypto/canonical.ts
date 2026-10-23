// Matrix canonical JSON: sorted Unicode keys, UTF-8 and safe integers only.
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value === "boolean" || typeof value === "string") return JSON.stringify(value);
  if (typeof value === "number") { if (!Number.isSafeInteger(value)) throw new Error("签名对象包含非安全整数"); return JSON.stringify(value); }
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (typeof value !== "object") throw new Error("签名对象不是有效 JSON");
  const object = value as Record<string, unknown>;
  const compare = (a: string, b: string) => { const left = Array.from(a, c => c.codePointAt(0)!), right = Array.from(b, c => c.codePointAt(0)!); for (let i = 0; i < Math.min(left.length, right.length); i++) if (left[i] !== right[i]) return left[i] - right[i]; return left.length - right.length; };
  return `{${Object.keys(object).sort(compare).map(key => `${JSON.stringify(key)}:${canonicalJson(object[key])}`).join(",")}}`;
}
