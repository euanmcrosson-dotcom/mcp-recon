/**
 * Unit tests for `parseServerSpec`. The other surfaces (`openClient`,
 * `enumerate`) are exercised by the integration test that points at
 * a real MCP server (see `enumerate.integration.test.ts`).
 */

import { describe, expect, it } from "vitest";

import { parseServerSpec } from "../transport.js";

describe("parseServerSpec", () => {
  it("parses a stdio command with no args", () => {
    expect(parseServerSpec("stdio:my-server")).toEqual({
      kind: "stdio",
      command: "my-server",
      args: [],
    });
  });

  it("parses a stdio command with multiple args", () => {
    expect(
      parseServerSpec("stdio:npx @modelcontextprotocol/server-filesystem /tmp"),
    ).toEqual({
      kind: "stdio",
      command: "npx",
      args: ["@modelcontextprotocol/server-filesystem", "/tmp"],
    });
  });

  it("parses a relative-path stdio command", () => {
    expect(parseServerSpec("stdio:./bin/recon-server --debug")).toEqual({
      kind: "stdio",
      command: "./bin/recon-server",
      args: ["--debug"],
    });
  });

  it("collapses multiple whitespace in stdio args", () => {
    expect(parseServerSpec("stdio:cmd   a   b")).toEqual({
      kind: "stdio",
      command: "cmd",
      args: ["a", "b"],
    });
  });

  it("parses an http URL", () => {
    expect(parseServerSpec("http://localhost:3000")).toEqual({
      kind: "http",
      url: "http://localhost:3000",
    });
  });

  it("parses an https URL", () => {
    expect(parseServerSpec("https://example.com:8443/mcp")).toEqual({
      kind: "http",
      url: "https://example.com:8443/mcp",
    });
  });

  it("throws on unrecognised prefix", () => {
    expect(() => parseServerSpec("ws://example.com")).toThrow(/unrecognised spec/);
    expect(() => parseServerSpec("just-some-text")).toThrow(/unrecognised spec/);
  });

  it("throws on empty stdio: prefix", () => {
    expect(() => parseServerSpec("stdio:")).toThrow(/requires a command/);
    expect(() => parseServerSpec("stdio:   ")).toThrow(/requires a command/);
  });
});
