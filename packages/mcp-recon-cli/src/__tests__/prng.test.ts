import { describe, expect, it } from "vitest";

import { Prng } from "../fuzz/prng.js";

describe("Prng", () => {
  it("is deterministic — same seed yields same sequence", () => {
    const a = new Prng(42);
    const b = new Prng(42);
    const seqA = Array.from({ length: 100 }, () => a.next());
    const seqB = Array.from({ length: 100 }, () => b.next());
    expect(seqA).toEqual(seqB);
  });

  it("different seeds yield different sequences", () => {
    const a = new Prng(42);
    const b = new Prng(43);
    const seqA = Array.from({ length: 10 }, () => a.next());
    const seqB = Array.from({ length: 10 }, () => b.next());
    expect(seqA).not.toEqual(seqB);
  });

  it("next() always in [0, 1)", () => {
    const p = new Prng(1);
    for (let i = 0; i < 1000; i++) {
      const v = p.next();
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(1);
    }
  });

  it("intInRange(lo, hi) stays in [lo, hi)", () => {
    const p = new Prng(7);
    for (let i = 0; i < 1000; i++) {
      const v = p.intInRange(5, 10);
      expect(v).toBeGreaterThanOrEqual(5);
      expect(v).toBeLessThan(10);
      expect(Number.isInteger(v)).toBe(true);
    }
  });

  it("intInRange throws if hi <= lo", () => {
    const p = new Prng(0);
    expect(() => p.intInRange(5, 5)).toThrow(/must be > lo/);
    expect(() => p.intInRange(5, 4)).toThrow(/must be > lo/);
  });

  it("pick chooses from non-empty arrays", () => {
    const p = new Prng(0);
    const arr = ["a", "b", "c"] as const;
    for (let i = 0; i < 100; i++) {
      const v = p.pick(arr);
      expect(arr).toContain(v);
    }
  });

  it("pick throws on empty array", () => {
    const p = new Prng(0);
    expect(() => p.pick([])).toThrow(/empty array/);
  });

  it("seed=0 still works (mixed in with constant)", () => {
    const p = new Prng(0);
    const v = p.next();
    expect(v).toBeGreaterThanOrEqual(0);
    expect(v).toBeLessThan(1);
  });
});
