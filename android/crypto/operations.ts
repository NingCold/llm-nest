import * as engine from "../../web/src/cloud/crypto/engine";
import * as content from "../../web/src/cloud/crypto/content";
import { context } from "./api";
import type { Conversation, Message, Snapshot, Page } from "../../web/src/cloud/types";


type Args = Record<string, any>;
const account = context.account;
let changed = false;
window.addEventListener("llmn-crypto-change", () => { changed = true; });
function mark(message: Message): Message {
  return message.sealed && !message.cryptoError ? { ...message, _decrypted: true } as Message : message;
}
async function pinned(conversation: Conversation) {
  return (await content.projectConversations([conversation], account))[0];
}
export async function dispatch(op: string, args: Args): Promise<object> {
  switch (op) {
    case "state": return {
      locked: !engine.cryptoDevice(account), hasDevice: engine.hasCryptoDevice(account),
      recoveryConfirmed: engine.recoveryConfirmed(account),
      ...(engine.cryptoDevice(account) ? await engine.deviceStatus() : {}),
    };
    case "projectSnapshot": {
      const snapshot: Snapshot = args.snapshot;
      if (snapshot.self.id !== account) throw new Error("账号已切换");
      const projected = await content.projectSnapshot(snapshot);
      return { ...projected, conversations: projected.conversations.map(c => ({ ...c, lastMessage: c.lastMessage ? mark(c.lastMessage) : null })) };
    }
    case "projectPage": {
      const conversation = await pinned(args.conversation);
      const page: Page = await content.projectPage(args.page, conversation);
      return { ...page, messages: page.messages.map(mark) };
    }
    case "prepareSend": {
      const conversation = await pinned(args.conversation);
      if (!conversation.encryption) throw new Error("此窗口尚未开启加密，不能准备密文命令");
      const messageId = crypto.randomUUID();
      const sealed = await content.encryptMessage(conversation, account, messageId, args.text);
      return { requestId: args.requestId, messageId, sealed, text: "", expectedConversationRevision: conversation.revision, mentions: [], replyTo: null, images: [], files: [], encryptedAttachments: [] };
    }
    case "enable": return await content.enableEncryption(await pinned(args.conversation), account) as object;
    case "sync": {
      if (engine.cryptoDevice(account)) await engine.syncCrypto();
      const result = { changed, locked: !engine.cryptoDevice(account) }; changed = false; return result;
    }
    case "unlock":
      if (typeof args.passphrase !== "string" || args.passphrase.length < 8) throw new Error("请输入至少 8 位的独立本机口令");
      await engine.unlockCrypto(account, args.passphrase, args.create ?? false); return dispatch("state", {});
    case "lock": engine.lockCrypto(); return dispatch("state", {});
    case "initialize": return engine.initializeIdentity();
    case "devices": return { devices: await engine.devices(args.account) };
    case "trust": await engine.trustDevice(args.account, args.deviceId, args.fingerprint, args.curve, args.master); return {};
    case "authorize": await engine.authorizeDevice(args.deviceId); return {};
    case "accept": await engine.acceptAuthorization(); return dispatch("state", {});
    case "createRecovery": return engine.createRecoveryFile();
    case "commitRecovery": await engine.commitRecoveryFile(args.file); return dispatch("state", {});
    case "restoreRecovery": await engine.restoreRecoveryFile(args.file); return dispatch("state", {});
    default: throw new Error("未知加密操作");
  }
}
