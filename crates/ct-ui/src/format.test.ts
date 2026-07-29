import { describe, expect, it } from "vitest";
import { formatBytes, formatPercent, formatTokens, projectName, shortId } from "./format";

describe("desktop formatting", () => {
  it("keeps dashboard numbers compact", () => {
    expect(formatTokens(982)).toBe("982");
    expect(formatTokens(12_345)).toBe("12.3k");
    expect(formatTokens(345_000)).toBe("345k");
    expect(formatPercent(0.734)).toBe("73%");
  });

  it("makes paths and identifiers scannable", () => {
    expect(projectName("C:\\work\\ContextTrace")).toBe("ContextTrace");
    expect(projectName("/work/context-trace/")).toBe("context-trace");
    expect(shortId("1234567890abcdef")).toBe("12345678");
    expect(formatBytes(1_572_864)).toBe("1.5 MB");
  });
});
