import { describe, expect, it } from "vitest";

import { CAVEATS_SCHEMA, planCaveats } from "../caveats/index.js";
import { renderCaveatsMarkdown } from "../caveats/render.js";
import type { CaveatBindings } from "../caveats/types.js";
import { CLASSIFICATION_SCHEMA } from "../classify/index.js";
import type { ClassificationResults } from "../classify/types.js";

const FROZEN_NOW = "2026-04-29T12:00:00.000Z";

const SAMPLE_CLASSIFICATION: ClassificationResults = {
	schema: CLASSIFICATION_SCHEMA,
	scanned_at: FROZEN_NOW,
	server: { name: "secure-filesystem-server", version: "0.2.0" },
	fuzz_informed: true,
	classifications: [
		{
			tool: "read_file",
			data_class: "filesystem",
			authority_level: "read",
			confused_deputy_candidate: false,
			confidence: 0.91,
			rationale: "name match → filesystem/read (0.70)",
			recommended_caveat:
				'tool == "read_file" AND caller == "<your-caller-id>" AND arg.path starts_with "<your-sandbox-prefix>/" AND now <= @<your-cap-expiry>  // READ filesystem',
		},
		{
			tool: "write_file",
			data_class: "filesystem",
			authority_level: "write",
			confused_deputy_candidate: true,
			confidence: 0.91,
			rationale: "name match → filesystem/write; cdc",
			recommended_caveat:
				'tool == "write_file" AND caller == "<your-caller-id>" AND arg.path starts_with "<your-sandbox-prefix>/" AND now <= @<your-cap-expiry>  // WRITE filesystem',
		},
		{
			tool: "sequentialthinking",
			data_class: "metadata",
			authority_level: "write",
			confused_deputy_candidate: true,
			confidence: 0.73,
			rationale: "cdc but no path arg",
			recommended_caveat:
				'tool == "sequentialthinking" AND caller == "<your-caller-id>" AND now <= @<your-cap-expiry>  // WRITE metadata',
		},
		{
			tool: "mystery_tool",
			data_class: "unknown",
			authority_level: "read",
			confused_deputy_candidate: false,
			confidence: 0.3,
			rationale: "no rules fired",
			recommended_caveat:
				'tool == "mystery_tool" AND caller == "<your-caller-id>" AND now <= @<your-cap-expiry>',
		},
	],
};

const FULL_BINDINGS: CaveatBindings = {
	caller: "agent:planner",
	sandbox_prefix: "/var/agent-sandbox/tenant-42",
	expiry: "2026-05-01T18:00:00.000Z",
};

describe("renderCaveatsMarkdown — shape + headings", () => {
	it("produces a non-empty string starting with the canonical heading", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		expect(typeof md).toBe("string");
		expect(md.length).toBeGreaterThan(0);
		expect(md.startsWith("# capnagent issuance plan")).toBe(true);
	});

	it("includes server identity and the document scanned_at timestamp", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		expect(md).toContain("secure-filesystem-server v0.2.0");
		expect(md).toContain(doc.scanned_at);
	});

	it("renders the bindings block with caller/sandbox_prefix/expiry values", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		expect(md).toContain("## Bindings");
		expect(md).toContain("agent:planner");
		expect(md).toContain("/var/agent-sandbox/tenant-42");
		expect(md).toContain("2026-05-01T18:00:00.000Z");
	});

	it("shows <unbound> for missing bindings", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, {});
		const md = renderCaveatsMarkdown(doc);
		// Three unbound bindings → at least three appearances of the marker.
		const occurrences = md.match(/<unbound>/g) ?? [];
		expect(occurrences.length).toBeGreaterThanOrEqual(3);
	});
});

describe("renderCaveatsMarkdown — summary + sections", () => {
	it("renders the summary counts (total / ready / flagged)", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		expect(md).toContain("## Summary");
		expect(md).toContain(`**Total plans:** ${doc.summary.total}`);
		expect(md).toContain(`**Ready to issue:** ${doc.summary.ready}`);
		expect(md).toContain(`**Flagged for review:** ${doc.summary.flagged}`);
	});

	it("lists every ready plan under 'Plans ready to issue'", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		const readyIdx = md.indexOf("## Plans ready to issue");
		expect(readyIdx).toBeGreaterThan(-1);
		const readySection = md.slice(readyIdx);
		for (const plan of doc.plans.filter((p) => !p.flagged)) {
			expect(readySection).toContain(`### ${plan.tool}`);
		}
	});

	it("lists every flagged plan under 'Flagged plans'", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		const flaggedIdx = md.indexOf("## ⚠ Flagged plans");
		expect(flaggedIdx).toBeGreaterThan(-1);
		// Slice up to the ready heading (or end of doc) to isolate the
		// flagged section.
		const readyIdx = md.indexOf("## Plans ready to issue");
		const flaggedSection =
			readyIdx > flaggedIdx
				? md.slice(flaggedIdx, readyIdx)
				: md.slice(flaggedIdx);
		for (const plan of doc.plans.filter((p) => p.flagged)) {
			expect(flaggedSection).toContain(`### ${plan.tool}`);
		}
	});
});

describe("renderCaveatsMarkdown — flag reasons + caveats + comments", () => {
	it("renders flag reasons as a bullet list for each flagged plan", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		for (const plan of doc.plans.filter((p) => p.flagged)) {
			// The plan's heading should be followed (within the section)
			// by each of its flag reasons as a `- ` bullet.
			const planIdx = md.indexOf(`### ${plan.tool}`);
			expect(planIdx).toBeGreaterThan(-1);
			// Bound the search to the start of the next plan-level (###) or
			// top-level (## but not ###) heading. Use line-anchored regex so
			// "## " inside text doesn't trip us.
			const after = md.slice(planIdx + 1);
			const nextPlan = after.search(/\n### /);
			const nextSection = after.search(/\n## [^#]/);
			const cutoffs = [nextPlan, nextSection, after.length].filter(
				(n) => n >= 0,
			);
			const sliceEnd = planIdx + 1 + Math.min(...cutoffs);
			const planSlice = md.slice(planIdx, sliceEnd);
			for (const reason of plan.flag_reasons) {
				expect(planSlice).toContain(`- \`${reason}\``);
			}
		}
	});

	it("renders each plan's caveats inside a fenced code block", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		for (const plan of doc.plans) {
			// Every caveat string must appear in the document.
			for (const c of plan.caveats) {
				expect(md).toContain(c);
			}
		}
		// Number of fenced blocks should equal number of plans (each
		// plan emits exactly one ``` ... ``` caveat block).
		const fences = md.match(/^```$/gm) ?? [];
		expect(fences.length).toBe(doc.plans.length * 2);
	});

	it("includes the plan comment when present and omits when absent", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		const md = renderCaveatsMarkdown(doc);
		// read_file has comment "READ filesystem"
		expect(md).toContain("**Comment:** READ filesystem");
		// mystery_tool has no comment — locate its section and confirm
		// no Comment line appears before the next plan/section.
		const mysteryIdx = md.indexOf("### mystery_tool");
		expect(mysteryIdx).toBeGreaterThan(-1);
		const after = md.slice(mysteryIdx + 1);
		const nextPlan = after.search(/\n### /);
		const nextSection = after.search(/\n## [^#]/);
		const cutoffs = [nextPlan, nextSection, after.length].filter((n) => n >= 0);
		const sliceEnd = mysteryIdx + 1 + Math.min(...cutoffs);
		const mysterySlice = md.slice(mysteryIdx, sliceEnd);
		expect(mysterySlice).not.toContain("**Comment:**");
	});
});

describe("renderCaveatsMarkdown — section omission", () => {
	it("omits the ready section when every plan is flagged", () => {
		// Empty bindings → every plan flagged for unsubstituted_placeholder.
		const doc = planCaveats(SAMPLE_CLASSIFICATION, {});
		expect(doc.summary.ready).toBe(0);
		const md = renderCaveatsMarkdown(doc);
		expect(md).not.toContain("## Plans ready to issue");
		expect(md).toContain("## ⚠ Flagged plans");
	});

	it("omits the flagged section when every plan is ready", () => {
		// Construct a classification with only a single high-confidence,
		// non-cdc, fully-substitutable tool — yields exactly one ready plan.
		const allReady: ClassificationResults = {
			schema: CLASSIFICATION_SCHEMA,
			scanned_at: FROZEN_NOW,
			server: { name: "tiny", version: "0.0.1" },
			fuzz_informed: false,
			classifications: [
				{
					tool: "read_file",
					data_class: "filesystem",
					authority_level: "read",
					confused_deputy_candidate: false,
					confidence: 0.91,
					rationale: "name match",
					recommended_caveat:
						'tool == "read_file" AND caller == "<your-caller-id>" AND arg.path starts_with "<your-sandbox-prefix>/" AND now <= @<your-cap-expiry>  // READ filesystem',
				},
			],
		};
		const doc = planCaveats(allReady, FULL_BINDINGS);
		expect(doc.summary.flagged).toBe(0);
		const md = renderCaveatsMarkdown(doc);
		expect(md).not.toContain("## ⚠ Flagged plans");
		expect(md).toContain("## Plans ready to issue");
	});
});

describe("renderCaveatsMarkdown — schema sanity", () => {
	it("works on a freshly-generated v0.1 caveats document (schema tag round-trip)", () => {
		const doc = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
		expect(doc.schema).toBe(CAVEATS_SCHEMA);
		// Should not throw and should produce something with the canonical heading.
		const md = renderCaveatsMarkdown(doc);
		expect(md).toMatch(/^# capnagent issuance plan/);
	});
});
