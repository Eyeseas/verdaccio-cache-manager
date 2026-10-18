import type { PreflightReport } from "./installPreflightLogic";

export interface LatestOutcome {
  name: string;
  expected_latest: string;
  actual_latest: string | null;
  action: string;
  message: string | null;
}

export interface BundleCompletion {
  latest: LatestOutcome[];
  verification: PreflightReport[];
  errors: string[];
}

export function summarizeLatest(outcomes: LatestOutcome[]) {
  const promoted = outcomes.filter((o) => o.action === "promoted").length;
  const differences = outcomes.filter((o) => o.actual_latest !== o.expected_latest).length;
  return `latest：推进 ${promoted} 个，${differences} 个与期望不同或未确认`;
}
