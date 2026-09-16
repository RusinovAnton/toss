import { describe, expect, it } from "vitest";
import { deviceEmoji, osFromModel } from "./device";

describe("deviceEmoji", () => {
  it("covers every device type in the protocol", () => {
    expect(deviceEmoji("desktop")).toBe("🖥️");
    expect(deviceEmoji("mobile")).toBe("📱");
    expect(deviceEmoji("web")).toBe("🌐");
    expect(deviceEmoji("headless")).toBe("⚙️");
    expect(deviceEmoji("server")).toBe("🗄️");
  });

  it("falls back to desktop, as the protocol requires", () => {
    expect(deviceEmoji(null)).toBe("🖥️");
    expect(deviceEmoji(undefined)).toBe("🖥️");
  });
});

describe("osFromModel", () => {
  it("recognises what the official app sends", () => {
    expect(osFromModel("macOS")).toBe("apple");
    expect(osFromModel("Windows")).toBe("windows");
    expect(osFromModel("Linux")).toBe("linux");
    expect(osFromModel("Samsung")).toBe("android");
    expect(osFromModel("iPhone")).toBe("apple");
    expect(osFromModel("iPad")).toBe("apple");
  });

  it("handles more phone brands and browsers", () => {
    expect(osFromModel("Pixel")).toBe("android");
    expect(osFromModel("Xiaomi")).toBe("android");
    expect(osFromModel("Samsung Internet")).toBe("android");
    expect(osFromModel("Microsoft Edge")).toBe("windows");
    expect(osFromModel("Safari")).toBe("apple");
  });

  it("is case insensitive", () => {
    expect(osFromModel("WINDOWS")).toBe("windows");
    expect(osFromModel("macos")).toBe("apple");
  });

  it("gives up cleanly", () => {
    expect(osFromModel(null)).toBe("unknown");
    expect(osFromModel("")).toBe("unknown");
    expect(osFromModel("Toaster 9000")).toBe("unknown");
  });
});
