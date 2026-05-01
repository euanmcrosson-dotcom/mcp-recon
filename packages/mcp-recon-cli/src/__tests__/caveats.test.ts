import { describe, expect, it } from "vitest";

import { CAVEATS_SCHEMA, planCaveats } from "../caveats/index.js";
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
      tool: "move_file",
      data_class: "filesystem",
      authority_level: "write",
      confused_deputy_candidate: true,
      confidence: 0.946,
      rationale: "name match; arg source + destination path-shaped",
      recommended_caveat:
        'tool == "move_file" AND caller == "<your-caller-id>" AND arg.source starts_with "<your-sandbox-prefix>/" AND arg.destination starts_with "<your-sandbox-prefix>/" AND now <= @<your-cap-expiry>  // WRITE filesystem',
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

describe("planCaveats — top-level shape", () => {
  it("emits a valid v0.1 caveats document", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    expect(out.schema).toBe(CAVEATS_SCHEMA);
    expect(out.scanned_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    expect(out.server).toEqual({
      name: "secure-filesystem-server",
      version: "0.2.0",
    });
    expect(out.bindings).toBe(FULL_BINDINGS);
  });

  it("emits one plan per classification entry", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    expect(out.plans).toHaveLength(SAMPLE_CLASSIFICATION.classifications.length);
  });

  it("summary counts ready vs flagged correctly", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    expect(out.summary.total).toBe(5);
    // read_file + write_file + move_file ready; sequentialthinking + mystery_tool flagged
    expect(out.summary.ready).toBe(3);
    expect(out.summary.flagged).toBe(2);
  });
});

describe("planCaveats — caveat splitting + comment stripping", () => {
  it("splits AND-joined caveats into individual entries", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.caveats).toHaveLength(4);
    expect(read_file.caveats[0]).toBe('tool == "read_file"');
    expect(read_file.caveats[1]).toBe('caller == "agent:planner"');
  });

  it("preserves the trailing // comment on the plan", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.comment).toBe("READ filesystem");
  });

  it("does not put // comment text into the caveat array", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    for (const plan of out.plans) {
      for (const c of plan.caveats) {
        expect(c).not.toContain("//");
        expect(c).not.toContain("READ filesystem");
        expect(c).not.toContain("WRITE filesystem");
      }
    }
  });

  it("handles entries with no trailing comment", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const mystery = out.plans.find((p) => p.tool === "mystery_tool")!;
    expect(mystery.comment).toBeUndefined();
  });
});

describe("planCaveats — placeholder substitution", () => {
  it("substitutes <your-caller-id> with bindings.caller (no double-quoting)", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.caveats).toContain('caller == "agent:planner"');
    expect(read_file.caveats.every((c) => !c.includes('""'))).toBe(true);
  });

  it("substitutes <your-sandbox-prefix> with bindings.sandbox_prefix", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.caveats.some((c) => c.includes("/var/agent-sandbox/tenant-42"))).toBe(true);
    expect(write.caveats.every((c) => !c.includes("<your-sandbox-prefix>"))).toBe(true);
  });

  it("substitutes <your-cap-expiry> with bindings.expiry", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    for (const plan of out.plans) {
      expect(plan.caveats.every((c) => !c.includes("<your-cap-expiry>"))).toBe(true);
    }
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.caveats.some((c) => c.includes("2026-05-01T18:00:00.000Z"))).toBe(true);
  });

  it("multi-arg path substitution: move_file gets both source and destination bound", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const move = out.plans.find((p) => p.tool === "move_file")!;
    expect(move.caveats).toHaveLength(5);
    expect(move.caveats.some((c) => c.includes('arg.source starts_with "/var/agent-sandbox/tenant-42/"'))).toBe(true);
    expect(move.caveats.some((c) => c.includes('arg.destination starts_with "/var/agent-sandbox/tenant-42/"'))).toBe(true);
  });

  it("leaves placeholder literal when binding is omitted", () => {
    const partial: CaveatBindings = {
      caller: "agent:planner",
      // sandbox_prefix omitted
      expiry: "2026-05-01T18:00:00.000Z",
    };
    const out = planCaveats(SAMPLE_CLASSIFICATION, partial);
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.caveats.some((c) => c.includes("<your-sandbox-prefix>"))).toBe(true);
  });
});

describe("planCaveats — flag rules", () => {
  it("flags classification_unknown", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const mystery = out.plans.find((p) => p.tool === "mystery_tool")!;
    expect(mystery.flagged).toBe(true);
    expect(mystery.flag_reasons).toContain("classification_unknown");
  });

  it("flags low_confidence", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const mystery = out.plans.find((p) => p.tool === "mystery_tool")!;
    expect(mystery.flag_reasons).toContain("low_confidence");
  });

  it("flags cdc_without_arg_constraint", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const seq = out.plans.find((p) => p.tool === "sequentialthinking")!;
    expect(seq.flagged).toBe(true);
    expect(seq.flag_reasons).toContain("cdc_without_arg_constraint");
  });

  it("does not flag cdc when arg.* constraint is present", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.flag_reasons).not.toContain("cdc_without_arg_constraint");
  });

  it("flags unsubstituted_placeholder", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, { caller: "agent:planner", expiry: "2026-05-01T18:00:00.000Z" });
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.flag_reasons).toContain("unsubstituted_placeholder");
  });

  it("returns empty bindings → flags every plan with placeholders", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, {});
    expect(out.summary.flagged).toBe(out.summary.total);
    for (const p of out.plans) {
      expect(p.flag_reasons).toContain("unsubstituted_placeholder");
    }
  });

  it("does not flag well-formed plans", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.flagged).toBe(false);
    expect(read_file.flag_reasons).toEqual([]);
  });
});

describe("planCaveats — purpose generation", () => {
  it("uses authority_level for non-cdc tools", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const read_file = out.plans.find((p) => p.tool === "read_file")!;
    expect(read_file.purpose).toBe("filesystem.read.read_file");
  });

  it("uses 'cdc' tag for confused-deputy candidates", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.purpose).toBe("filesystem.cdc.write_file");
  });
});

describe("planCaveats — per_tool_overrides", () => {
  it("appends override caveats after substituted ones", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, {
      ...FULL_BINDINGS,
      per_tool_overrides: {
        write_file: ['arg.path matches "\\\\.txt$"'],
      },
    });
    const write = out.plans.find((p) => p.tool === "write_file")!;
    expect(write.caveats[write.caveats.length - 1]).toBe('arg.path matches "\\\\.txt$"');
  });

  it("can rescue a cdc-without-arg-constraint plan via override", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, {
      ...FULL_BINDINGS,
      per_tool_overrides: {
        sequentialthinking: ["arg.totalThoughts <= 50"],
      },
    });
    const seq = out.plans.find((p) => p.tool === "sequentialthinking")!;
    expect(seq.flag_reasons).not.toContain("cdc_without_arg_constraint");
  });
});

describe("planCaveats — pass-through fields", () => {
  it("preserves data_class, authority_level, confused_deputy_candidate from classification", () => {
    const out = planCaveats(SAMPLE_CLASSIFICATION, FULL_BINDINGS);
    const move = out.plans.find((p) => p.tool === "move_file")!;
    expect(move.data_class).toBe("filesystem");
    expect(move.authority_level).toBe("write");
    expect(move.confused_deputy_candidate).toBe(true);
  });
});
