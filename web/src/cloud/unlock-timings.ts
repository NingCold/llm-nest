const stages = {
  "llmn.crypto.prewarm": ["sdkReady", "indexedDbReady", "holdReady", "failed"],
  "llmn.crypto.unlock": ["sdkImport", "sdkInit", "storeOpen", "storePrepare", "storeMetadataRead", "storeMetadata", "storeCipher", "storeDatabase", "machineLoad", "deviceRegister", "trackedUsers", "deviceStatus", "statusIdentity", "statusCrossSigning", "statusOwnDeviceTrust", "unlockReady", "backgroundSync"],
  "llmn.chat.startup": ["initRequest", "snapshotProjection", "firstSnapshotReady", "firstSnapshotCommit", "firstSnapshotFrame"],
};
const allowed = new Set(Object.entries(stages).flatMap(([prefix, names]) => names.map(name => `${prefix}.${name}`)));
export interface UnlockTiming { name: string; startTime: number; duration: number }

// Copy only known numeric measurements; never carry entry detail or other metadata.
export function selectUnlockTimings(entries: readonly PerformanceEntry[]): UnlockTiming[] {
  return entries.filter(entry => entry.entryType === "measure" && allowed.has(entry.name)
    && Number.isFinite(entry.startTime) && entry.startTime >= 0
    && Number.isFinite(entry.duration) && entry.duration >= 0)
    .map(({ name, startTime, duration }) => ({ name, startTime, duration }));
}
