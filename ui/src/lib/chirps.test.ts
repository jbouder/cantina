import { describe, expect, it } from "vitest";
import { phrase } from "./chirps";

describe("chirps", () => {
  it("rises for a reply and falls for an error, briefly", () => {
    const seeded = (() => {
      let x = 0.3;
      return () => {
        x = (x * 9301 + 0.49297) % 1;
        return x;
      };
    })();
    const reply = phrase("reply", seeded);
    expect(reply.length).toBeGreaterThanOrEqual(3);
    for (const [from = 0, to = 0, length = 0] of reply) {
      expect(to).toBeGreaterThan(from);
      expect(length).toBeLessThan(0.1);
    }
    const error = phrase("error");
    for (const [from = 0, to = 0] of error) expect(to).toBeLessThan(from);
    const total = error.reduce((t, [, , l = 0]) => t + l, 0);
    expect(total).toBeLessThan(0.6);
  });
});
