import { describe, expect, it } from "vitest";

import { coerceBigIntsToNumbers } from "./coerceBigInt";

describe("coerceBigIntsToNumbers", () => {
  it("converts top-level and nested BigInt values to numbers", () => {
    const input = {
      schema_version: 2,
      plugins: [
        { id: "a", artifact: { size: 447416n, sha256: "abc" } },
        { id: "b", artifact: { size: 100n } },
      ],
    };
    const out = coerceBigIntsToNumbers(input);
    expect(typeof out.plugins[0].artifact.size).toBe("number");
    expect(out.plugins[0].artifact.size).toBe(447416);
    expect(out.plugins[1].artifact.size).toBe(100);
    // Non-bigint values pass through untouched.
    expect(out.schema_version).toBe(2);
    expect(out.plugins[0].artifact.sha256).toBe("abc");
  });

  it("leaves plain numbers, strings, null, and booleans untouched", () => {
    const input = { a: 1, b: "x", c: null, d: true, e: [1, "y", false] };
    expect(coerceBigIntsToNumbers(input)).toEqual(input);
  });

  it("coerces BigInt inside arrays", () => {
    expect(coerceBigIntsToNumbers([1n, 2n, 3])).toEqual([1, 2, 3]);
  });
});
