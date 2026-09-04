import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

import { DotNetRandom, installDotNetRandom } from "./dotnet-random.mjs";

describe("DotNetRandom", () => {
  // The same vectors crates/brink-runtime/src/rng.rs pins for DotNetRng —
  // .NET System.Random, seed 0.
  it("reproduces .NET System.Random's seed-0 sequence", () => {
    const rng = new DotNetRandom(0);
    assert.deepEqual(
      [rng.next(), rng.next(), rng.next(), rng.next(), rng.next()],
      [1559595546, 1755192844, 1649316166, 1198642031, 442452829],
    );
  });

  it("stays non-negative for negative seeds and across a long run", () => {
    for (const seed of [-1, -2147483648, 42, 2147483647]) {
      const rng = new DotNetRandom(seed);
      for (let i = 0; i < 1000; i++) {
        const v = rng.next();
        assert.ok(v >= 0 && v <= 2147483647, `seed ${seed}: ${v}`);
      }
    }
  });

  it("wraps a JS number seed to a C# int before seeding", () => {
    // 2^32 + 7 wraps to 7; a fractional seed floors first (inkjs already
    // calls Math.floor at the shuffle site, this keeps the other sites safe).
    const a = new DotNetRandom(4294967303);
    const b = new DotNetRandom(7);
    assert.equal(a.next(), b.next());
    assert.equal(new DotNetRandom(7.9).next(), new DotNetRandom(7).next());
  });

  it("installs itself as the PRNG inkjs's Story reads", () => {
    const prngModule = installDotNetRandom();
    assert.equal(prngModule.PRNG, DotNetRandom);
    // Story.js holds the same module object, so it sees the swap.
    const require = createRequire(import.meta.url);
    assert.equal(require("inkjs/engine/PRNG").PRNG, DotNetRandom);
  });
});
