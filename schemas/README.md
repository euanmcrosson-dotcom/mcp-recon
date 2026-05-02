# mcp-recon JSON Schemas

Formal [JSON Schema](https://json-schema.org/) definitions for every artifact `mcp-recon` emits. These files are the **machine-readable wire-format contract** that downstream consumers — in any language, not just TypeScript — can validate documents against.

| Schema tag | File | Source-of-truth shape |
|---|---|---|
| `mcp-recon/v0.1/inventory` | [`inventory.v0.1.json`](./inventory.v0.1.json) | `ToolInventory` in [`packages/mcp-recon-cli/src/enumerate.ts`](../packages/mcp-recon-cli/src/enumerate.ts) |
| `mcp-recon/v0.1/fuzz` | [`fuzz.v0.1.json`](./fuzz.v0.1.json) | `FuzzResults` in [`packages/mcp-recon-cli/src/fuzz/types.ts`](../packages/mcp-recon-cli/src/fuzz/types.ts) |
| `mcp-recon/v0.1/classification` | [`classification.v0.1.json`](./classification.v0.1.json) | `ClassificationResults` in [`packages/mcp-recon-cli/src/classify/types.ts`](../packages/mcp-recon-cli/src/classify/types.ts) |
| `mcp-recon/v0.1/caveats` | [`caveats.v0.1.json`](./caveats.v0.1.json) | `CaveatsResults` in `packages/mcp-recon-cli/src/caveats/types.ts` |

## Why these files exist

The TypeScript `interface` is the canonical definition of each artifact's shape — that is what we maintain by hand and what the implementation uses. But:

- A TypeScript interface is unusable from Python, Rust, Go, or any other language that consumes mcp-recon output.
- A schema-tag string in a document (`"schema": "mcp-recon/v0.1/inventory"`) tells the consumer what they are looking at, but does not tell them how to validate it.

These JSON Schema files close that gap. A consumer can pin to a specific URL (`$id` is the GitHub raw URL of the file at `master`), validate with `ajv`, `jsonschema`, `python-jsonschema`, or any 2020-12-compatible validator, and reject malformed documents before processing.

## Version-pinning convention

**A published schema is never edited.** The TypeScript interface is the source of truth for the *shape*; the JSON Schema in `master` is the source of truth for the *historical wire format at version v0.1*. Once a document tagged `mcp-recon/v0.1/X` ships in a release, the corresponding `X.v0.1.json` is frozen — third parties have started validating against it.

A breaking shape change adds a new file:

- `inventory.v0.1.json` — current
- `inventory.v0.2.json` — added when v0.2 ships, valid alongside v0.1

The schema tag inside the document carries the version (`mcp-recon/v0.2/inventory`); the filename mirrors that. Forward-compatible additions (new optional fields) are not breaking and could in principle land in v0.1, but in practice we prefer to bump rather than relax `additionalProperties: false` — that way the contract stays exact.

## How to validate a document

With [`ajv-cli`](https://www.npmjs.com/package/ajv-cli):

```sh
npx ajv-cli validate \
  --spec=draft2020 \
  -s schemas/inventory.v0.1.json \
  -d examples/public-servers/server-filesystem/inventory.json
```

With Python's `jsonschema`:

```python
import json
from jsonschema import Draft202012Validator

schema = json.load(open("schemas/inventory.v0.1.json"))
doc    = json.load(open("examples/public-servers/server-filesystem/inventory.json"))
Draft202012Validator(schema).validate(doc)
```

In a TypeScript test (this repo uses `ajv` + `ajv-formats` — see [`packages/mcp-recon-cli/src/__tests__/schemas.test.ts`](../packages/mcp-recon-cli/src/__tests__/schemas.test.ts)):

```ts
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const ajv = new Ajv2020({ strict: false });
addFormats(ajv);
const validate = ajv.compile(schemaJson);
if (!validate(doc)) throw new Error(JSON.stringify(validate.errors));
```

## When the TypeScript and the JSON Schema disagree

The TypeScript interface wins. If you change a field in the TypeScript type and forget to update the JSON Schema, the schemas test in this repo will catch it on the example document — but a hand-encoded JSON Schema is a derived artifact and can drift. If you spot a mismatch:

1. Treat the TypeScript interface as authoritative.
2. Update the JSON Schema only if it is the *current unreleased* version.
3. If the schema has shipped, the fix is a new `vX.Y` file — do not edit the published one.
