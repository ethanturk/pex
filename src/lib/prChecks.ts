import type { PRCheck } from "@/lib/api";

export type PRCheckRollupStatus = "pass" | "fail" | "running" | "none";

export interface PRCheckRollup {
  status: PRCheckRollupStatus;
  tooltip: string;
  requiredText: string;
  optionalText: string;
}

export function isPrCheckPending(status: string): boolean {
  return status === "queued" || status === "running";
}

export function isPrCheckFailed(status: string): boolean {
  return status === "rejected" || status === "broken";
}

export function getPrCheckRollup(checks: PRCheck[]): PRCheckRollup {
  if (checks.length === 0) {
    return {
      status: "none",
      tooltip: "No configured build checks.",
      requiredText: "No configured build checks",
      optionalText: "",
    };
  }

  const required = checks.filter((check) => check.isRequired);
  const optional = checks.filter((check) => !check.isRequired);
  const failed = checks.filter((check) => isPrCheckFailed(check.status));
  const pending = checks.filter((check) => isPrCheckPending(check.status));
  const requiredPassed = required.filter((check) => check.status === "approved").length;
  const requiredFailed = required.filter((check) => isPrCheckFailed(check.status)).length;
  const requiredPending = required.filter((check) => isPrCheckPending(check.status)).length;
  const optionalNotRun = optional.filter((check) => check.status === "notApplicable").length;

  const status: PRCheckRollupStatus =
    failed.length > 0 ? "fail" : pending.length > 0 ? "running" : "pass";

  const requiredText =
    required.length === 0
      ? "No required checks"
      : requiredFailed > 0
        ? `${requiredFailed} of ${required.length} required builds failed`
        : requiredPending > 0
          ? `${requiredPending} of ${required.length} required builds running now`
          : `${requiredPassed} of ${required.length} required builds passed`;
  const optionalText =
    optionalNotRun > 0
      ? `${optionalNotRun} optional ${optionalNotRun === 1 ? "check" : "checks"} not yet run`
      : optional.length > 0
        ? `${optional.length} optional`
        : "";

  const headline =
    status === "fail"
      ? `Build checks failed: ${failed.length} ${failed.length === 1 ? "check" : "checks"} failed.`
      : status === "running"
        ? `Build checks running: ${pending.length} ${pending.length === 1 ? "check is" : "checks are"} running or queued.`
        : "Build checks passed.";
  const summary = optionalText ? `${requiredText}; ${optionalText}.` : `${requiredText}.`;
  const details = checks
    .map((check) => `${check.isRequired ? "Required" : "Optional"}: ${check.name} (${check.status})`)
    .join("\n");

  return {
    status,
    tooltip: `${headline} ${summary}\n${details}`,
    requiredText,
    optionalText,
  };
}

/// Turn a raw PR-checks error into a user-facing message. Azure DevOps' branch
/// policy evaluations endpoint isn't reachable with a Personal Access Token, so
/// an auth failure here usually means "sign in with OAuth instead".
export function describeChecksError(e: unknown): string {
  const msg = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  if (/\b401\b|\b403\b|unauthorized|forbidden|TF400813|sign[\s-]?in/i.test(msg)) {
    return `${msg} — Azure DevOps PR checks (branch-policy evaluations) aren't accessible with a Personal Access Token; sign in with OAuth to see them.`;
  }
  return msg;
}
