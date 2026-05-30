/**
 * Seedable deterministic PRNG.
 *
 * The fuzzer's reproducibility contract requires that running with the
 * same seed produces bit-identical inputs. A `Math.random()`-style
 * non-seeded source breaks that.
 *
 * Implementation: mulberry32. 32-bit state, decent statistical
 * quality for non-cryptographic use, fits in 8 lines. Period 2^32 —
 * more than enough for a fuzz budget that's typically <10^4 calls.
 *
 * Reference: https://gist.github.com/tommyettinger/46a3a48676ef5ec4a4e4
 */

import { DEFAULT_FUZZ_SEED } from "./schema.js";

export class Prng {
	private state: number;

	constructor(seed: number = DEFAULT_FUZZ_SEED) {
		// Allow seed = 0 by mixing in a constant.
		this.state = (seed | 0) ^ 0x6d2b79f5;
	}

	/** Uniform [0, 1). */
	next(): number {
		this.state = (this.state + 0x6d2b79f5) | 0;
		let t = Math.imul(this.state ^ (this.state >>> 15), 1 | this.state);
		t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	}

	/** Integer in [lo, hi). */
	intInRange(lo: number, hi: number): number {
		if (hi <= lo) {
			throw new Error(`Prng.intInRange: hi (${hi}) must be > lo (${lo})`);
		}
		return Math.floor(this.next() * (hi - lo)) + lo;
	}

	/** Pick a random element of an array. Throws on empty. */
	pick<T>(items: readonly T[]): T {
		if (items.length === 0) {
			throw new Error("Prng.pick: empty array");
		}
		const item = items[this.intInRange(0, items.length)];
		if (item === undefined) {
			// unreachable given the in-range index, but TS doesn't know that
			throw new Error("Prng.pick: undefined slot");
		}
		return item;
	}

	/** Bias coin — true with probability p (0..1). */
	bool(p = 0.5): boolean {
		return this.next() < p;
	}
}
