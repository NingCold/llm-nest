export type ImageSupport = "supported" | "unsupported" | "unknown"
export interface ImageInputCapability { support: ImageSupport; source: "config" | "catalog" | "builtin" | "unknown" }
export const imageSupportLabels: Record<ImageSupport, string> = { supported: "支持图片", unsupported: "不支持图片", unknown: "图片能力未知" }
export const imageSourceLabels: Record<ImageInputCapability["source"], string> = { config: "人工配置", catalog: "同步目录", builtin: "内置目录", unknown: "暂无信息" }
