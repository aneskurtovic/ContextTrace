import assert from "node:assert/strict";
import test from "node:test";
import { assessPushChecks } from "./verify-main-validation.mjs";

const status = (name, state = "success", target_url = "https://ci.aneskurtovic.com/repos/5/pipeline/84/1") => ({
  context: `ci/woodpecker/push/${name}`,
  state,
  target_url,
});

test("accepts the three successful push checks for the tagged commit", () => {
  const result = assessPushChecks([status("frontend"), status("rust"), status("windows")]);
  assert.equal(result.state, "success");
});

test("waits for missing or pending checks", () => {
  assert.equal(assessPushChecks([status("frontend"), status("rust")]).state, "pending");
  assert.equal(assessPushChecks([status("frontend"), status("rust", "pending"), status("windows")]).state, "pending");
});

test("rejects a failed check or a context that does not point to this Woodpecker server", () => {
  assert.equal(assessPushChecks([status("frontend"), status("rust", "failure"), status("windows")]).state, "failure");
  assert.equal(assessPushChecks([
    status("frontend"), status("rust"), status("windows", "success", "https://example.com/other-ci"),
  ]).state, "failure");
});
