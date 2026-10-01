import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const checks = ["frontend", "rust", "windows"];
const contextPrefix = "ci/woodpecker/push/";
const apiRoot = "https://api.github.com/repos/aneskurtovic/ContextTrace";
const pollIntervalMs = 120_000;
const timeoutMs = 60 * 60_000;

// GitHub's combined-status endpoint returns the latest status per context for
// the requested commit. The Windows push workflow runs only on main, so its
// success binds all three checks to a commit that was validated there.
export function assessPushChecks(statuses) {
  if (!Array.isArray(statuses)) throw new Error("GitHub returned no commit statuses array.");
  const results = checks.map((name) => {
    const status = statuses.find((item) => item.context === `${contextPrefix}${name}`);
    if (!status) return { name, state: "pending" };
    let validWoodpeckerUrl = false;
    try {
      const url = new URL(status.target_url);
      validWoodpeckerUrl = url.protocol === "https:" && url.hostname === "ci.aneskurtovic.com"
        && /^\/repos\/\d+\/pipeline\/\d+\/\d+$/.test(url.pathname);
    } catch {
      // A context without a link to this Woodpecker server is not release evidence.
    }
    if (!validWoodpeckerUrl) return { name, state: "invalid" };
    return { name, state: status.state };
  });
  const failed = results.some(({ state }) => !["success", "pending"].includes(state));
  const passed = results.every(({ state }) => state === "success");
  return { state: failed ? "failure" : passed ? "success" : "pending", results };
}

async function main() {
  const sha = process.env.CI_COMMIT_SHA;
  const tag = process.env.CI_COMMIT_TAG;
  if (!/^[0-9a-f]{40}$/.test(sha ?? "") || !/^v\d+\.\d+\.\d+$/.test(tag ?? "")) {
    throw new Error("A version tag and its full commit SHA are required from Woodpecker.");
  }
  const deadline = Date.now() + timeoutMs;
  let previous = "";
  while (true) {
    const response = await fetch(`${apiRoot}/commits/${sha}/status`, {
      headers: {
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "ContextTrace-Woodpecker-Release-Gate",
      },
      signal: AbortSignal.timeout(15_000),
    });
    if (!response.ok) throw new Error(`GitHub commit status request failed: HTTP ${response.status}.`);
    const payload = await response.json();
    if (payload.sha !== sha) throw new Error("GitHub returned statuses for a different commit.");
    const result = assessPushChecks(payload.statuses);
    const summary = result.results.map(({ name, state }) => `${name}: ${state}`).join(", ");
    if (summary !== previous) console.log(`Woodpecker push checks for ${sha}: ${summary}`);
    previous = summary;
    if (result.state === "success") {
      console.log(`Release gate passed for ${tag}.`);
      return;
    }
    if (result.state === "failure") throw new Error(`Release gate rejected ${tag}: ${summary}`);
    if (Date.now() >= deadline) throw new Error(`Timed out waiting for main push checks: ${summary}`);
    await delay(pollIntervalMs);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
