// Synthetic-only entry: never import the product App, API, engine or storage.
const store = "llmn-synthetic-sdk-benchmark-v1";
const passphrase = "synthetic-performance-fixture-only";
const user = "@performance-fixture:llmn", device = "performance-fixture";
const params = new URLSearchParams(location.search);
const mode = params.get("mode");
const debug = params.get("devtools") === "1";
type Sample = { round: number; stages: Record<string, number>; valid: boolean };
async function main() {
  const stages: Record<string, number> = {};
  let start = performance.now();
  const sdk = await import("@matrix-org/matrix-sdk-crypto-wasm");
  stages.sdkImport = performance.now() - start;
  start = performance.now(); await sdk.initAsync(); stages.sdkInit = performance.now() - start;
  const uid = new sdk.UserId(user), did = new sdk.DeviceId(device);
  const samples: Sample[] = [];
  try {
    if (mode === "seed") {
      if ((await indexedDB.databases()).some(db => db.name?.startsWith(store))) throw new Error("Fixture already exists");
      const machine = await sdk.OlmMachine.initialize(uid, did, store, passphrase);
      try {
        const bootstrap = await machine.bootstrapCrossSigning(false);
        const signing = JSON.parse(bootstrap.uploadSigningKeysRequest.body);
        const signatures = JSON.parse(bootstrap.uploadSignaturesRequest.body);
        const uploadedDevice = JSON.parse(bootstrap.uploadKeysRequest!.body).device_keys;
        const signedDevice = signatures[user][device];
        signedDevice.signatures[user] = { ...uploadedDevice.signatures[user], ...signedDevice.signatures[user] };
        // Simulate only the fixture's own directory response, wholly inside this SDK store.
        await machine.updateTrackedUsers([new sdk.UserId(user)]);
        while (true) {
          const pending = await machine.outgoingRequests();
          if (!pending.length) break;
          for (const outgoing of pending) {
            if (outgoing.type === sdk.RequestType.KeysUpload) {
              await machine.markRequestAsSent(outgoing.id!, outgoing.type, JSON.stringify({ one_time_key_counts: { signed_curve25519: 50 } }));
            } else if (outgoing.type === sdk.RequestType.KeysQuery) {
              await machine.markRequestAsSent(outgoing.id!, outgoing.type, JSON.stringify({
                device_keys: { [user]: { [device]: signedDevice } },
                master_keys: { [user]: signing.master_key }, self_signing_keys: { [user]: signing.self_signing_key },
                user_signing_keys: { [user]: signing.user_signing_key }, failures: {},
              }));
            } else throw new Error("Unexpected fixture request");
          }
        }
        const identity = await machine.getIdentity(uid);
        try {
          const cross = await machine.crossSigningStatus();
          try { samples.push({ round: 0, stages: {}, valid: !!identity?.isVerified() && "trustsOurOwnDevice" in identity && await identity.trustsOurOwnDevice() && cross.hasMaster }); }
          finally { cross.free(); }
        }
        finally { identity?.free(); }
      } finally { machine.close(); }
    } else if (mode === "grow" || mode === "measure") {
      const names = new Set((await indexedDB.databases()).map(db => db.name));
      if (!names.has(`${store}::matrix-sdk-crypto-meta`) || !names.has(`${store}::matrix-sdk-crypto`)) throw new Error("Seed fixture first");
      if (mode === "grow") {
        const handle = await sdk.StoreHandle.open(store, passphrase);
        let machine;
        try { machine = await sdk.OlmMachine.initFromStore(uid, did, handle); }
        finally { handle.free(); }
        try {
          let uploaded = 0;
          start = performance.now();
          for (let batch = 0; batch < 100; batch++) {
            const lists = new sdk.DeviceLists([], []);
            try { await machine.receiveSyncChanges("[]", lists, new Map([["signed_curve25519", 0]]), new Set(["signed_curve25519"])); }
            finally { lists.free(); }
            while (true) {
              const pending = await machine.outgoingRequests();
              if (!pending.length) break;
              for (const outgoing of pending) {
                if (outgoing.type !== sdk.RequestType.KeysUpload) throw new Error("Unexpected growth request");
                uploaded += Object.keys(JSON.parse(outgoing.body).one_time_keys ?? {}).length;
                await machine.markRequestAsSent(outgoing.id!, outgoing.type, JSON.stringify({ one_time_key_counts: { signed_curve25519: 50 } }));
              }
            }
          }
          stages.grow = performance.now() - start;
          const identity = await machine.getIdentity(uid);
          try {
            const cross = await machine.crossSigningStatus();
            try { samples.push({ round: 0, stages: {}, valid: uploaded >= 5000 && (await machine.outgoingRequests()).length === 0 && !!identity?.isVerified() && "trustsOurOwnDevice" in identity && await identity.trustsOurOwnDevice() && cross.hasMaster }); }
            finally { cross.free(); }
          } finally { identity?.free(); }
        } finally { machine.close(); }
      } else {
        for (let round = 1; round <= 4; round++) {
          const times: Record<string, number> = {};
          const openStart = performance.now();
          let cipherStart: number | undefined, databaseStart: number | undefined;
          const logger = { debug(...args: unknown[]) {
            const has = (text: string) => args.some(value => typeof value === "string" && value.includes(text));
            if (has("decrypting store cipher")) cipherStart = performance.now();
            if (has("opening main store")) {
              databaseStart = performance.now();
              if (cipherStart !== undefined) times.storeCipher = databaseStart - cipherStart;
            }
          }, info() {}, warn() {}, error() {} };
          const handle = await sdk.StoreHandle.open(store, passphrase, logger);
          const opened = performance.now();
          times.storeOpen = opened - openStart;
          if (databaseStart !== undefined) times.storeDatabase = opened - databaseStart;
          let machine;
          try { machine = await sdk.OlmMachine.initFromStore(uid, did, handle); }
          finally { handle.free(); }
          times.machineLoad = performance.now() - opened;
          try {
            // updateTrackedUsers consumes its UserId array; keep the shared ID alive.
            start = performance.now(); await machine.updateTrackedUsers([new sdk.UserId(user)]); times.trackedUsers = performance.now() - start;
            start = performance.now(); const identity = await machine.getIdentity(uid); times.statusIdentity = performance.now() - start;
            try {
              start = performance.now(); const cross = await machine.crossSigningStatus(); times.statusCrossSigning = performance.now() - start;
              try {
                start = performance.now();
                const trusted = !!identity && "trustsOurOwnDevice" in identity && await identity.trustsOurOwnDevice();
                times.statusOwnDeviceTrust = performance.now() - start;
                start = performance.now();
                const trustedAgain = !!identity && "trustsOurOwnDevice" in identity && await identity.trustsOurOwnDevice();
                times.statusOwnDeviceTrustCached = performance.now() - start;
                samples.push({ round, stages: times, valid: !!identity?.isVerified() && trusted && trustedAgain && cross.hasMaster });
              } finally { cross.free(); }
            } finally { identity?.free(); }
          } finally { machine.close(); }
        }
      }
    } else throw new Error("Expected seed, grow or measure mode");
  } finally { uid.free(); did.free(); }
  return { mode, debug, success: true, stages, samples };
}
const run = () => main().catch(() => ({ mode, debug, success: false, stages: {}, samples: [] })).then(async result => {
  document.body.textContent = result.success ? "Synthetic benchmark complete" : "Synthetic benchmark failed";
  const response = await fetch("/result", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(result) });
  if (response.ok) location.href = "/done";
});
if (params.get("manual") === "1") {
  const button = document.createElement("button");
  button.textContent = "开始合成测量";
  button.onclick = () => { button.disabled = true; void run(); };
  document.body.replaceChildren(button);
} else void run();
