import { describe, expect, it } from "vitest";
import { formatBytes } from "./format";

describe("formatBytes", () => {
  it("formats bytes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(999)).toBe("999 B");
  });
  it("formats kilobytes and megabytes", () => {
    expect(formatBytes(1500)).toBe("1.5 KB");
    expect(formatBytes(42_000_000)).toBe("42 MB");
  });
  it("formats gigabytes", () => {
    expect(formatBytes(3_200_000_000)).toBe("3.2 GB");
  });
  it("handles garbage", () => {
    expect(formatBytes(-1)).toBe("0 B");
    expect(formatBytes(NaN)).toBe("0 B");
  });
});
